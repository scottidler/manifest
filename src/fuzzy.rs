// src/fuzzy.rs

#![allow(dead_code)]

use std::collections::HashMap;
use regex::Regex;
use glob::Pattern;

#[derive(Debug)]
pub struct FuzzyError(pub String);

impl std::fmt::Display for FuzzyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "FuzzyError: {}", self.0)
    }
}

impl std::error::Error for FuzzyError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchType {
    Exact,
    IgnoreCase,
    Prefix,
    Suffix,
    Contains,
    Glob,
    Regex,
}

pub const DEFAULT_MATCH_TYPES: [MatchType; 4] = [
    MatchType::Exact,
    MatchType::IgnoreCase,
    MatchType::Prefix,
    MatchType::Contains,
];

fn match_str(item: &str, pattern: &str, mt: MatchType) -> bool {
    match mt {
        MatchType::Exact => item == pattern,
        MatchType::IgnoreCase => item.eq_ignore_ascii_case(pattern),
        MatchType::Prefix => item.starts_with(pattern),
        MatchType::Suffix => item.ends_with(pattern),
        MatchType::Contains => item.contains(pattern),
        MatchType::Glob => {
            if let Ok(p) = Pattern::new(pattern) {
                p.matches(item)
            } else {
                false
            }
        }
        MatchType::Regex => {
            if let Ok(re) = Regex::new(pattern) {
                re.is_match(item)
            } else {
                false
            }
        }
    }
}

pub trait Fuzz {
    type Output;
    fn include(self, patterns: &[String]) -> Self::Output;
    fn exclude(self, patterns: &[String]) -> Self::Output;
    fn defuzz(self) -> Self::Output;
}

impl Fuzz for Vec<String> {
    type Output = Vec<String>;

    fn include(self, patterns: &[String]) -> Vec<String> {
        if patterns.is_empty() || patterns.iter().any(|p| p == "*") {
            return self;
        }
        let items = self;
        for &mt in DEFAULT_MATCH_TYPES.iter() {
            let results: Vec<String> = items
                .iter()
                .cloned()
                .filter(|item| {
                    patterns.iter().any(|pattern| match_str(item, pattern, mt))
                })
                .collect();
            if !results.is_empty() {
                return results;
            }
        }
        Vec::new()
    }

    fn exclude(self, patterns: &[String]) -> Vec<String> {
        if patterns.is_empty() || patterns.iter().any(|p| p == "*") {
            return Vec::new();
        }
        let items = self;
        for &mt in DEFAULT_MATCH_TYPES.iter() {
            let results: Vec<String> = items
                .iter()
                .cloned()
                .filter(|item| {
                    patterns.iter().all(|pattern| !match_str(item, pattern, mt))
                })
                .collect();
            if !results.is_empty() {
                return results;
            }
        }
        Vec::new()
    }

    fn defuzz(self) -> Vec<String> {
        self
    }
}

impl<T: Clone + PartialEq> Fuzz for HashMap<String, T> {
    type Output = HashMap<String, T>;

    fn include(self, patterns: &[String]) -> HashMap<String, T> {
        if patterns.is_empty() || patterns.iter().any(|p| p == "*") {
            return self;
        }
        let keys: Vec<String> = self.keys().cloned().collect();
        for &mt in DEFAULT_MATCH_TYPES.iter() {
            let matched_keys: Vec<String> = keys
                .iter()
                .cloned()
                .filter(|key| {
                    patterns.iter().any(|pattern| match_str(key, pattern, mt))
                })
                .collect();
            if !matched_keys.is_empty() {
                let cloned = self.clone();
                let result: HashMap<String, T> = cloned
                    .into_iter()
                    .filter(|(key, _)| matched_keys.contains(key))
                    .collect();
                return result;
            }
        }
        HashMap::new()
    }

    fn exclude(self, patterns: &[String]) -> HashMap<String, T> {
        if patterns.is_empty() || patterns.iter().any(|p| p == "*") {
            return HashMap::new();
        }
        let keys: Vec<String> = self.keys().cloned().collect();
        for &mt in DEFAULT_MATCH_TYPES.iter() {
            let remaining_keys: Vec<String> = keys
                .iter()
                .cloned()
                .filter(|key| {
                    patterns.iter().all(|pattern| !match_str(key, pattern, mt))
                })
                .collect();
            if !remaining_keys.is_empty() {
                let cloned = self.clone();
                let result: HashMap<String, T> = cloned
                    .into_iter()
                    .filter(|(key, _)| remaining_keys.contains(key))
                    .collect();
                return result;
            }
        }
        HashMap::new()
    }

    fn defuzz(self) -> HashMap<String, T> {
        self
    }
}

pub fn fuzzy<T>(obj: T) -> T
where
    T: Fuzz<Output = T>,
{
    obj
}

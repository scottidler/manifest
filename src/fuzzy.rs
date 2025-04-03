// src/fuzzy.rs

#![allow(dead_code)]

use std::collections::HashMap;
use regex::Regex;
use glob::Pattern;
//use eyre::{Result, eyre};

/// An error type for fuzzy matching issues.
#[derive(Debug)]
pub struct FuzzyError(pub String);

impl std::fmt::Display for FuzzyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "FuzzyError: {}", self.0)
    }
}

impl std::error::Error for FuzzyError {}

/// The various matching strategies.
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

/// The default match types (in order) to try when matching a key.
pub const DEFAULT_MATCH_TYPES: [MatchType; 4] = [
    MatchType::Exact,
    MatchType::IgnoreCase,
    MatchType::Prefix,
    MatchType::Contains,
];

/// Given an item string and a pattern, return true if the item matches the pattern using the specified match type.
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

/// Returns true if the given key matches any of the provided patterns using one or more match types.
fn key_matches(key: &str, patterns: &[String], match_types: &[MatchType]) -> bool {
    for pattern in patterns {
        for &mt in match_types {
            if match_str(key, pattern, mt) {
                return true;
            }
        }
    }
    false
}

/// A fuzzy list built from a list of strings.
#[derive(Debug, Clone)]
pub struct FuzzyList {
    items: Vec<String>,
}

impl FuzzyList {
    /// Create a new FuzzyList.
    pub fn new(items: Vec<String>) -> Self {
        Self { items }
    }

    /// Return a new FuzzyList containing only those items whose string value matches at least one pattern.
    pub fn include(&self, patterns: &[String]) -> Self {
        let filtered = self
            .items
            .iter()
            .filter(|item| key_matches(item, patterns, &DEFAULT_MATCH_TYPES))
            .cloned()
            .collect();
        FuzzyList { items: filtered }
    }

    /// Return a new FuzzyList excluding items that match any of the given patterns.
    pub fn exclude(&self, patterns: &[String]) -> Self {
        let filtered = self
            .items
            .iter()
            .filter(|item| !key_matches(item, patterns, &DEFAULT_MATCH_TYPES))
            .cloned()
            .collect();
        FuzzyList { items: filtered }
    }

    /// Unwrap the underlying Vec.
    pub fn defuzz(self) -> Vec<String> {
        self.items
    }

    /// Borrow a slice of the underlying Vec.
    pub fn as_slice(&self) -> &[String] {
        &self.items
    }
}

/// A generic fuzzy dictionary. This replaces the previous YAML-specific FuzzyDict.
/// It holds a HashMap with String keys and any type T as values. Its include/exclude methods
/// filter entries based on fuzzy matching of the keys.
#[derive(Debug, Clone)]
pub struct FuzzyDict<T> {
    pub items: HashMap<String, T>,
}

impl<T: Clone> FuzzyDict<T> {
    /// Create a new FuzzyDict from a HashMap.
    pub fn new(items: HashMap<String, T>) -> Self {
        Self { items }
    }

    /// Include only entries whose keys match ANY of the given patterns.
    /// If any pattern is "*" then all keys are included.
    /// Optionally override the match types; otherwise the default match types are used.
    pub fn include(&self, patterns: &[String], match_types: Option<&[MatchType]>) -> Self {
        let mts = match_types.unwrap_or(&DEFAULT_MATCH_TYPES);
        let filtered = self.items.iter()
            .filter(|(key, _)| {
                if patterns.iter().any(|p| p == "*") {
                    true
                } else {
                    key_matches(key, patterns, mts)
                }
            })
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        Self { items: filtered }
    }

    /// Exclude entries whose keys match ANY of the given patterns.
    /// If any pattern is "*" then all keys are excluded.
    pub fn exclude(&self, patterns: &[String], match_types: Option<&[MatchType]>) -> Self {
        let mts = match_types.unwrap_or(&DEFAULT_MATCH_TYPES);
        let filtered = self.items.iter()
            .filter(|(key, _)| {
                if patterns.iter().any(|p| p == "*") {
                    false
                } else {
                    !key_matches(key, patterns, mts)
                }
            })
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        Self { items: filtered }
    }

    /// Consume self and return the underlying HashMap.
    pub fn defuzz(self) -> HashMap<String, T> {
        self.items
    }
}

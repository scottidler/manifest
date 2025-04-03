// src/fuzzy.rs

#![allow(dead_code)]

use std::collections::HashMap;
use regex::Regex;
use glob::Pattern;

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
/// (Note: This mirrors the Python default: EXACT, IGNORECASE, PREFIX, CONTAINS.)
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

/// The unified trait for fuzzy matching. This trait is implemented for both Vec<String>
/// and HashMap<String, T> so that the same interface can be used.
pub trait Fuzz {
    type Output;
    /// Include only items whose string representation matches at least one pattern.
    /// (This replicates the Python logic: for each match type in order, filter items using that
    /// match function—returning the first non-empty set.)
    fn include(self, patterns: &[String]) -> Self::Output;
    /// Exclude items that match any of the given patterns.
    fn exclude(self, patterns: &[String]) -> Self::Output;
    /// Return the underlying value (akin to "defuzzing" in Python).
    fn defuzz(self) -> Self::Output;
}

/// Implementation for Vec<String>.
impl Fuzz for Vec<String> {
    type Output = Vec<String>;

    fn include(self, patterns: &[String]) -> Vec<String> {
        // If any pattern is "*" return all items.
        if patterns.iter().any(|p| p == "*") {
            return self;
        }
        let items = self; // consume self; we use items by reference below
        for &mt in DEFAULT_MATCH_TYPES.iter() {
            // For each match type, filter items: for an item to match, at least one pattern must match
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
        if patterns.iter().any(|p| p == "*") {
            return Vec::new();
        }
        let items = self;
        for &mt in DEFAULT_MATCH_TYPES.iter() {
            let results: Vec<String> = items
                .iter()
                .cloned()
                .filter(|item| {
                    // For exclude, an item is kept only if it does NOT match any pattern for the given match type.
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

/// Implementation for HashMap<String, T>.
impl<T: Clone + PartialEq> Fuzz for HashMap<String, T> {
    type Output = HashMap<String, T>;

    fn include(self, patterns: &[String]) -> HashMap<String, T> {
        if patterns.iter().any(|p| p == "*") {
            return self;
        }
        // First, collect the keys.
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
                // Build a new HashMap from keys that matched.
                // (Clone self so we can filter without consuming it.)
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
        if patterns.iter().any(|p| p == "*") {
            return HashMap::new();
        }
        let keys: Vec<String> = self.keys().cloned().collect();
        for &mt in DEFAULT_MATCH_TYPES.iter() {
            let remaining_keys: Vec<String> = keys
                .iter()
                .cloned()
                .filter(|key| {
                    // Keep key only if for the given match type, none of the patterns match.
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

/// A generic fuzzy entrypoint. It simply returns the object passed in,
/// which then can be used to call include/exclude/defuzz.
/// For example:
///     let filtered = fuzzy(my_vec).include(&patterns).defuzz();
pub fn fuzzy<T>(obj: T) -> T
where
    T: Fuzz<Output = T>,
{
    obj
}

// --- Example tests (optional) ---
//
// #[cfg(test)]
// mod tests {
//     use super::*;
//     #[test]
//     fn test_vec_include() {
//         let items = vec!["apple".to_string(), "banana".to_string(), "apricot".to_string()];
//         let patterns = vec!["app".to_string()];
//         let filtered = items.include(&patterns);
//         // For DEFAULT_MATCH_TYPES, "Exact" won’t match, but "Prefix" will: "apple" and "apricot"
//         assert_eq!(filtered, vec!["apple".to_string(), "apricot".to_string()]);
//     }
//
//     #[test]
//     fn test_map_include() {
//         let mut map = HashMap::new();
//         map.insert("foo".to_string(), 1);
//         map.insert("bar".to_string(), 2);
//         let patterns = vec!["ba".to_string()];
//         let filtered = map.include(&patterns);
//         assert!(filtered.contains_key("bar"));
//         assert!(!filtered.contains_key("foo"));
//     }
// }

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

/// The unified trait for fuzzy matching. This trait is implemented for both Vec<String>
/// and HashMap<String, T> so that the same interface can be used.
pub trait FuzzyOps {
    type Output;
    /// Include only items whose string representation matches at least one pattern.
    fn include(self, patterns: &[String]) -> Self::Output;
    /// Exclude items that match any of the given patterns.
    fn exclude(self, patterns: &[String]) -> Self::Output;
    /// Return the underlying value (akin to "defuzzing" in Python).
    fn defuzz(self) -> Self::Output;
}

/// Implement FuzzyOps for Vec<String> so that a list of strings can be filtered.
impl FuzzyOps for Vec<String> {
    type Output = Vec<String>;

    fn include(self, patterns: &[String]) -> Vec<String> {
        // If any pattern is "*" return all items.
        if patterns.iter().any(|p| p == "*") {
            self
        } else {
            self.into_iter()
                .filter(|item| key_matches(item, patterns, &DEFAULT_MATCH_TYPES))
                .collect()
        }
    }

    fn exclude(self, patterns: &[String]) -> Vec<String> {
        if patterns.iter().any(|p| p == "*") {
            Vec::new()
        } else {
            self.into_iter()
                .filter(|item| !key_matches(item, patterns, &DEFAULT_MATCH_TYPES))
                .collect()
        }
    }

    fn defuzz(self) -> Vec<String> {
        self
    }
}

/// Implement FuzzyOps for HashMap<String, T> where T is Clone.
/// This allows fuzzy filtering based on the keys.
impl<T: Clone> FuzzyOps for HashMap<String, T> {
    type Output = HashMap<String, T>;

    fn include(self, patterns: &[String]) -> HashMap<String, T> {
        if patterns.iter().any(|p| p == "*") {
            self
        } else {
            self.into_iter()
                .filter(|(key, _)| key_matches(key, patterns, &DEFAULT_MATCH_TYPES))
                .collect()
        }
    }

    fn exclude(self, patterns: &[String]) -> HashMap<String, T> {
        if patterns.iter().any(|p| p == "*") {
            HashMap::new()
        } else {
            self.into_iter()
                .filter(|(key, _)| !key_matches(key, patterns, &DEFAULT_MATCH_TYPES))
                .collect()
        }
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
    T: FuzzyOps<Output = T>,
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

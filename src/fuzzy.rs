// src/fuzzy.rs

#![allow(dead_code)]

use std::collections::HashMap;
use serde_yaml::Value;
use eyre::{Result, eyre};
use glob::Pattern;

/// A manual error type that we can wrap with `eyre!` as needed.
#[derive(Debug)]
pub struct FuzzyError(pub String);

impl std::fmt::Display for FuzzyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid fuzzy type: {}", self.0)
    }
}

impl std::error::Error for FuzzyError {}

/// The “fuzzy” object for a list of strings
#[derive(Debug, Clone)]
pub struct FuzzyList {
    items: Vec<String>,
}

impl FuzzyList {
    /// Create a new FuzzyList from a `Vec<String>`
    pub fn new(items: Vec<String>) -> Self {
        Self { items }
    }

    /// Keep only items that match ANY of the patterns
    pub fn include(&self, patterns: &[String]) -> Self {
        let filtered = self
            .items
            .iter()
            .filter(|item| matches_any(item, patterns))
            .cloned()
            .collect();
        FuzzyList { items: filtered }
    }

    /// Keep only items that *do not* match ANY of the patterns (exclude them).
    pub fn exclude(&self, patterns: &[String]) -> Self {
        let filtered = self
            .items
            .iter()
            .filter(|item| !matches_any(item, patterns))
            .cloned()
            .collect();
        FuzzyList { items: filtered }
    }

    /// Return the final underlying vector of strings
    pub fn defuzz(self) -> Vec<String> {
        self.items
    }

    /// Borrowing version if you prefer (returns a slice).
    /// But typically `defuzz()` is enough.
    pub fn as_slice(&self) -> &[String] {
        &self.items
    }
}

/// The “fuzzy” object for a dictionary with string keys
#[derive(Debug, Clone)]
pub struct FuzzyDict {
    items: HashMap<String, Value>,
}

impl FuzzyDict {
    /// Create a new `FuzzyDict` from a `HashMap<String, Value>`
    pub fn new(items: HashMap<String, Value>) -> Self {
        Self { items }
    }

    /// Keep only entries whose keys match ANY of the patterns
    pub fn include(&self, patterns: &[String]) -> Self {
        let filtered = self
            .items
            .iter()
            .filter(|(key, _)| matches_any(key, patterns))
            .map(|(k,v)| (k.clone(), v.clone()))
            .collect();
        FuzzyDict { items: filtered }
    }

    /// Keep only entries whose keys *do not* match ANY of the patterns
    pub fn exclude(&self, patterns: &[String]) -> Self {
        let filtered = self
            .items
            .iter()
            .filter(|(key, _)| !matches_any(key, patterns))
            .map(|(k,v)| (k.clone(), v.clone()))
            .collect();
        FuzzyDict { items: filtered }
    }

    /// Return the final underlying `HashMap<String, Value>`
    pub fn defuzz(self) -> HashMap<String, Value> {
        self.items
    }
}

/// An enum that can hold either a `FuzzyList` or `FuzzyDict`.
#[derive(Debug, Clone)]
pub enum FuzzyValue {
    List(FuzzyList),
    Dict(FuzzyDict),
}

impl FuzzyValue {
    /// If this is a list, call `.include(...)`. If it’s a dict, call `.include(...)` on the keys.
    /// If you want separate `include_list()` vs `include_dict()`, do so.
    pub fn include(self, patterns: &[String]) -> Self {
        match self {
            FuzzyValue::List(fl) => FuzzyValue::List(fl.include(patterns)),
            FuzzyValue::Dict(fd) => FuzzyValue::Dict(fd.include(patterns)),
        }
    }

    /// Exclude matching items or keys
    pub fn exclude(self, patterns: &[String]) -> Self {
        match self {
            FuzzyValue::List(fl) => FuzzyValue::List(fl.exclude(patterns)),
            FuzzyValue::Dict(fd) => FuzzyValue::Dict(fd.exclude(patterns)),
        }
    }

    /// Return the final “unwrapped” data:
    /// - If a list, a `Vec<String>`.
    /// - If a dict, a `HashMap<String, Value>`.
    pub fn defuzz(self) -> Defuzzed {
        match self {
            FuzzyValue::List(fl) => Defuzzed::List(fl.defuzz()),
            FuzzyValue::Dict(fd) => Defuzzed::Dict(fd.defuzz()),
        }
    }
}

/// The final unwrapped data after `.defuzz()`
#[derive(Debug)]
pub enum Defuzzed {
    List(Vec<String>),
    Dict(HashMap<String, Value>),
}

/// The top-level “fuzzy(...)” function, akin to `fuzzy(obj)` in Python:
/// - If `value` is an array of strings, returns a `FuzzyValue::List`.
/// - If `value` is an object (map) with string keys, returns `FuzzyValue::Dict`.
/// - Otherwise, error.
pub fn fuzzy(value: &Value) -> Result<FuzzyValue> {
    match value {
        Value::Sequence(seq) => {
            let mut string_items = Vec::new();
            for elem in seq {
                match elem {
                    Value::String(s) => string_items.push(s.clone()),
                    _ => {
                        return Err(eyre!(FuzzyError(
                            "Sequence contains non-string item".to_string()
                        )));
                    }
                }
            }
            Ok(FuzzyValue::List(FuzzyList::new(string_items)))
        }
        Value::Mapping(map) => {
            let mut hm = HashMap::new();
            for (k, v) in map {
                let key = match k {
                    Value::String(s) => s.clone(),
                    _ => {
                        return Err(eyre!(FuzzyError(
                            "Mapping contains non-string key".to_string()
                        )));
                    }
                };
                hm.insert(key, v.clone());
            }
            Ok(FuzzyValue::Dict(FuzzyDict::new(hm)))
        }
        _ => Err(eyre!(FuzzyError(
            "Value is not sequence or mapping".to_string()
        ))),
    }
}

/// Return true if `item` matches ANY pattern, via:
/// 1) exact
/// 2) ignore-case
/// 3) prefix
/// 4) substring
/// 5) glob
fn matches_any(item: &str, patterns: &[String]) -> bool {
    for pat in patterns {
        if matches_fuzzy(item, pat) {
            return true;
        }
    }
    false
}

fn matches_fuzzy(item: &str, pat: &str) -> bool {
    // 1) exact
    if item == pat {
        return true;
    }
    // 2) ignore case
    if item.eq_ignore_ascii_case(pat) {
        return true;
    }
    // 3) prefix
    if item.starts_with(pat) {
        return true;
    }
    // 4) substring
    if item.contains(pat) {
        return true;
    }
    // 5) glob
    if let Ok(g) = Pattern::new(pat) {
        if g.matches(item) {
            return true;
        }
    }
    false
}

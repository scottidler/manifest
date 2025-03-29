// src/config.rs

use serde::Deserialize;
use std::{collections::HashMap, fs};
use eyre::Result;
use walkdir::WalkDir;

/// If a link spec says `recursive: true`, we walk all files under <cwd>/<srcpath> 
/// and generate a set of pairs (absolute_source_file, final_dest).
pub fn recursive_links(srcpath: &str, dstpath: &str, cwd: &str, home: &str) -> Vec<(String, String)> {
    let mut results = Vec::new();
    let joined = format!("{}/{}", cwd, srcpath);
    for entry in WalkDir::new(&joined)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        let rel = match entry.path().strip_prefix(&joined) {
            Ok(r) => r,
            Err(_) => continue,
        };
        let source_abs = entry.path().to_string_lossy().to_string();

        // If we found <cwd>/<srcpath>/foo/bar.txt,
        // 'rel' = foo/bar.txt => produce <dstpath>/foo/bar.txt
        let new_dest = format!("{}/{}", dstpath, rel.display());
        let final_dest = new_dest.replace("~", home).replace("$HOME", home);

        results.push((source_abs, final_dest));
    }
    results
}

// For GitHub, e.g. "owner/repo" -> object that might have "link", "script", etc.
#[derive(Debug, Deserialize)]
pub struct GithubRepoSpec {
    pub link: Option<HashMap<String, String>>,
    pub script: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct LinkSpec {
    pub recursive: Option<bool>,
    #[serde(flatten)]
    pub items: HashMap<String, String>,
}

#[derive(Debug, Deserialize)]
pub struct PpaSpec {
    pub items: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
pub struct PackageSpec {
    pub items: Option<Vec<String>>,
}

/// The top-level YAML structure, mirroring your Python approach
#[derive(Debug, Deserialize)]
pub struct ManifestSpec {
    pub link: Option<LinkSpec>,
    pub ppa: Option<PpaSpec>,
    pub pkg: Option<PackageSpec>,
    pub apt: Option<PackageSpec>,
    pub dnf: Option<PackageSpec>,
    pub npm: Option<PackageSpec>,
    pub pip3: Option<PackageSpec>,
    pub pipx: Option<PackageSpec>,
    pub flatpak: Option<PackageSpec>,
    pub cargo: Option<PackageSpec>,

    #[serde(default)]
    pub github: HashMap<String, GithubRepoSpec>,

    #[serde(default)]
    pub script: HashMap<String, String>,

    #[serde(default)]
    pub verbose: bool,

    #[serde(default)]
    pub errors: bool,
}

/// A simple wrapper so we can do `Manifest::load_from_file(...)`
#[derive(Debug)]
pub struct Manifest {
    pub spec: ManifestSpec,
}

impl Manifest {
    pub fn load_from_file(path: &str) -> Result<Self> {
        let contents = fs::read_to_string(path)?;
        let spec: ManifestSpec = serde_yaml::from_str(&contents)?;
        Ok(Self { spec })
    }
}

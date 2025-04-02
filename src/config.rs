// src/config.rs

use eyre::Result;
use serde::{Deserialize, Serialize};
use serde_yaml::from_reader;
use std::collections::HashMap;
use std::io::Read;

/// The top-level structure for the entire manifest file.
/// This matches the Python's original layout but renamed for clarity.
/// Everything is optional in the sense that the user may not specify certain sections.
/// We rely on `#[serde(default)]` for missing fields.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ManifestSpec {
    #[serde(default)]
    pub verbose: bool,
    #[serde(default)]
    pub errors: bool,

    #[serde(default)]
    pub link: LinkSpec,
    #[serde(default)]
    pub ppa: PpaSpec,
    #[serde(default)]
    pub pkg: PkgSpec,
    #[serde(default)]
    pub apt: AptSpec,
    #[serde(default)]
    pub dnf: DnfSpec,
    #[serde(default)]
    pub npm: NpmSpec,
    #[serde(default)]
    pub pip3: Pip3Spec,
    #[serde(default)]
    pub pipx: PipxSpec,
    #[serde(default)]
    pub flatpak: FlatpakSpec,
    #[serde(default)]
    pub cargo: CargoSpec,
    #[serde(default)]
    pub github: GithubSpec,
    #[serde(default)]
    pub script: ScriptSpec,
}

/// For linking files:
/// - `recursive`: optional bool
/// - `items`: a map from src->dst
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct LinkSpec {
    #[serde(default)]
    pub recursive: bool,
    /// If `recursive=true`, we might interpret these src/dst differently.
    /// But either way, store them in `items`.
    #[serde(flatten)]
    pub items: HashMap<String, String>,
}

/// For adding PPAs: just a list of items
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct PpaSpec {
    #[serde(default)]
    pub items: Vec<String>,
}

/// For top-level pkg
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct PkgSpec {
    #[serde(default)]
    pub items: Vec<String>,
}

/// For APT: just a list of items
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct AptSpec {
    #[serde(default)]
    pub items: Vec<String>,
}

/// For DNF
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct DnfSpec {
    #[serde(default)]
    pub items: Vec<String>,
}

/// For NPM
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct NpmSpec {
    #[serde(default)]
    pub items: Vec<String>,
}

/// For pip3: we have an items vec plus "distutils" folded in. We'll unify them at runtime.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Pip3Spec {
    #[serde(default)]
    pub items: Vec<String>,
    #[serde(default)]
    pub distutils: Vec<String>,
}

/// For pipx
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct PipxSpec {
    #[serde(default)]
    pub items: Vec<String>,
}

/// For flatpak
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct FlatpakSpec {
    #[serde(default)]
    pub items: Vec<String>,
}

/// For cargo
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct CargoSpec {
    #[serde(default)]
    pub items: Vec<String>,
}

/// For script, we store "items" as a HashMap of scriptName->scriptBody
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ScriptSpec {
    #[serde(default)]
    pub items: HashMap<String, String>,
}

/// For GitHub, we keep a field "repopath" plus a HashMap of repoName->RepoSpec
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct GithubSpec {
    #[serde(default = "default_repopath")]
    pub repopath: String,
    /// The user wants "items" for everything, so let's rename "repos" -> "items":
    #[serde(default)]
    #[serde(flatten)]
    pub items: HashMap<String, RepoSpec>,
}

fn default_repopath() -> String {
    "repos".to_string()
}

/// Each named repository is a struct that can have link + script
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct RepoSpec {
    #[serde(default)]
    pub link: LinkSpec,
    #[serde(default)]
    pub script: ScriptSpec,
}

/// Helper to load the Manifest from a reader
pub fn load_manifest_spec<R: Read>(r: R) -> Result<ManifestSpec> {
    let parsed: ManifestSpec = from_reader(r)?;
    Ok(parsed)
}

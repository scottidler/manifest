// src/config.rs

use eyre::Result;
use serde::{Deserialize, Serialize};
use serde_yaml::from_reader;
use std::collections::HashMap;
use std::io::Read;

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

#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
pub struct LinkSpec {
    #[serde(default)]
    pub recursive: bool,
    #[serde(flatten)]
    pub items: HashMap<String, String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct PpaSpec {
    #[serde(default)]
    pub items: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct PkgSpec {
    #[serde(default)]
    pub items: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct AptSpec {
    #[serde(default)]
    pub items: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct DnfSpec {
    #[serde(default)]
    pub items: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct NpmSpec {
    #[serde(default)]
    pub items: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Pip3Spec {
    #[serde(default)]
    pub items: Vec<String>,
    #[serde(default)]
    pub distutils: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct PipxSpec {
    #[serde(default)]
    pub items: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct FlatpakSpec {
    #[serde(default)]
    pub items: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct CargoSpec {
    #[serde(default)]
    pub items: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
pub struct ScriptSpec {
    #[serde(default)]
    #[serde(flatten)]
    pub items: HashMap<String, String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct GithubSpec {
    #[serde(default = "default_repopath")]
    pub repopath: String,
    #[serde(default)]
    #[serde(flatten)]
    pub items: HashMap<String, RepoSpec>,
}

fn default_repopath() -> String {
    "repos".to_string()
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
pub struct RepoSpec {
    #[serde(default)]
    pub link: LinkSpec,
    #[serde(default)]
    pub cargo: Vec<String>,
    #[serde(default)]
    pub script: ScriptSpec,
}

pub fn load_manifest_spec<R: Read>(r: R) -> Result<ManifestSpec> {
    let parsed: ManifestSpec = from_reader(r)?;
    Ok(parsed)
}

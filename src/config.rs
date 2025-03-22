use eyre::Result;
use serde::Deserialize;
use std::{collections::HashMap, fs::File, path::Path};

#[derive(Debug, Deserialize)]
pub struct Section {
    pub items: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct LinkSection {
    #[serde(default)]
    pub recursive: bool,
    #[serde(flatten)]
    pub paths: HashMap<String, String>,
}

#[derive(Debug, Deserialize)]
pub struct GithubRepo {
    pub link: Option<HashMap<String, String>>,
}

#[derive(Debug, Deserialize)]
pub struct ManifestSpec {
    pub verbose: Option<bool>,
    pub link: Option<LinkSection>,
    pub ppa: Option<Section>,
    pub apt: Option<Section>,
    pub dnf: Option<Section>,
    pub npm: Option<Section>,
    #[serde(rename = "pip3")]
    pub pip3: Option<Section>,
    pub pipx: Option<Section>,
    pub flatpak: Option<Section>,
    pub cargo: Option<Section>,
    pub github: Option<HashMap<String, GithubRepo>>,
    pub script: Option<HashMap<String, String>>,
}

impl ManifestSpec {
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self> {
        let file = File::open(path)?;
        let spec: ManifestSpec = serde_yaml::from_reader(file)?;
        Ok(spec)
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_manifest_spec_default() {
        let spec = ManifestSpec::default();
        assert!(!spec.verbose);
        assert!(!spec.errors);
        assert!(spec.link.items.is_empty());
        assert!(spec.ppa.items.is_empty());
        assert!(spec.pkg.items.is_empty());
        assert!(spec.apt.items.is_empty());
        assert!(spec.dnf.items.is_empty());
        assert!(spec.npm.items.is_empty());
        assert!(spec.pip3.items.is_empty());
        assert!(spec.pip3.distutils.is_empty());
        assert!(spec.pipx.items.is_empty());
        assert!(spec.flatpak.items.is_empty());
        assert!(spec.cargo.items.is_empty());
        assert!(spec.github.items.is_empty());
        assert!(spec.script.items.is_empty());
    }

    #[test]
    fn test_link_spec_deserialization() {
        let yaml = r#"
recursive: true
HOME: $HOME
"bin/test": "~/bin/test"
"#;
        let spec: LinkSpec = serde_yaml::from_str(yaml).unwrap();
        assert!(spec.recursive);
        assert_eq!(spec.items.len(), 2);
        assert_eq!(spec.items.get("HOME"), Some(&"$HOME".to_string()));
        assert_eq!(spec.items.get("bin/test"), Some(&"~/bin/test".to_string()));
    }

    #[test]
    fn test_link_spec_serialization() {
        let mut items = HashMap::new();
        items.insert("src".to_string(), "dst".to_string());
        let spec = LinkSpec {
            recursive: true,
            items,
        };
        let yaml = serde_yaml::to_string(&spec).unwrap();
        assert!(yaml.contains("recursive: true"));
        assert!(yaml.contains("src: dst"));
    }

    #[test]
    fn test_ppa_spec_deserialization() {
        let yaml = r#"
items:
  - git-core/ppa
  - mkusb/ppa
"#;
        let spec: PpaSpec = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(spec.items.len(), 2);
        assert!(spec.items.contains(&"git-core/ppa".to_string()));
        assert!(spec.items.contains(&"mkusb/ppa".to_string()));
    }

    #[test]
    fn test_pkg_spec_deserialization() {
        let yaml = r#"
items:
  - jq
  - vim
  - htop
"#;
        let spec: PkgSpec = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(spec.items.len(), 3);
        assert!(spec.items.contains(&"jq".to_string()));
        assert!(spec.items.contains(&"vim".to_string()));
        assert!(spec.items.contains(&"htop".to_string()));
    }

    #[test]
    fn test_apt_spec_deserialization() {
        let yaml = r#"
items:
  - fuse3
  - ldap-utils
  - fonts-powerline
"#;
        let spec: AptSpec = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(spec.items.len(), 3);
        assert!(spec.items.contains(&"fuse3".to_string()));
        assert!(spec.items.contains(&"ldap-utils".to_string()));
        assert!(spec.items.contains(&"fonts-powerline".to_string()));
    }

    #[test]
    fn test_dnf_spec_deserialization() {
        let yaml = r#"
items:
  - the_silver_searcher
  - gcc
  - libffi-devel
"#;
        let spec: DnfSpec = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(spec.items.len(), 3);
        assert!(spec.items.contains(&"the_silver_searcher".to_string()));
        assert!(spec.items.contains(&"gcc".to_string()));
        assert!(spec.items.contains(&"libffi-devel".to_string()));
    }

    #[test]
    fn test_npm_spec_deserialization() {
        let yaml = r#"
items:
  - diff-so-fancy
  - wt-cli
  - auth0-deploy-cli
"#;
        let spec: NpmSpec = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(spec.items.len(), 3);
        assert!(spec.items.contains(&"diff-so-fancy".to_string()));
        assert!(spec.items.contains(&"wt-cli".to_string()));
        assert!(spec.items.contains(&"auth0-deploy-cli".to_string()));
    }

    #[test]
    fn test_pip3_spec_deserialization() {
        let yaml = r#"
items:
  - argh
  - numpy
  - twine
distutils:
  - Cython
  - pexpect
"#;
        let spec: Pip3Spec = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(spec.items.len(), 3);
        assert_eq!(spec.distutils.len(), 2);
        assert!(spec.items.contains(&"argh".to_string()));
        assert!(spec.items.contains(&"numpy".to_string()));
        assert!(spec.items.contains(&"twine".to_string()));
        assert!(spec.distutils.contains(&"Cython".to_string()));
        assert!(spec.distutils.contains(&"pexpect".to_string()));
    }

    #[test]
    fn test_pipx_spec_deserialization() {
        let yaml = r#"
items:
  - doit
  - mypy
  - awscli
"#;
        let spec: PipxSpec = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(spec.items.len(), 3);
        assert!(spec.items.contains(&"doit".to_string()));
        assert!(spec.items.contains(&"mypy".to_string()));
        assert!(spec.items.contains(&"awscli".to_string()));
    }

    #[test]
    fn test_flatpak_spec_deserialization() {
        let yaml = r#"
items:
  - org.gnome.GTG
  - org.gnome.BreakTimer
  - com.github.hugolabe.Wike
"#;
        let spec: FlatpakSpec = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(spec.items.len(), 3);
        assert!(spec.items.contains(&"org.gnome.GTG".to_string()));
        assert!(spec.items.contains(&"org.gnome.BreakTimer".to_string()));
        assert!(spec.items.contains(&"com.github.hugolabe.Wike".to_string()));
    }

    #[test]
    fn test_cargo_spec_deserialization() {
        let yaml = r#"
items:
  - bat
  - cargo-expand
  - du-dust
"#;
        let spec: CargoSpec = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(spec.items.len(), 3);
        assert!(spec.items.contains(&"bat".to_string()));
        assert!(spec.items.contains(&"cargo-expand".to_string()));
        assert!(spec.items.contains(&"du-dust".to_string()));
    }

    #[test]
    fn test_script_spec_deserialization() {
        let yaml = r#"
rust: |
  curl https://sh.rustup.rs -sSf | sh
docker: |
  curl -fsSL https://download.docker.com/linux/ubuntu/gpg | sudo apt-key add -
  sudo add-apt-repository "deb [arch=amd64] https://download.docker.com/linux/ubuntu $(lsb_release -cs) stable"
"#;
        let spec: ScriptSpec = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(spec.items.len(), 2);
        assert!(spec.items.contains_key("rust"));
        assert!(spec.items.contains_key("docker"));
        assert!(spec.items.get("rust").unwrap().contains("curl https://sh.rustup.rs"));
        assert!(spec.items.get("docker").unwrap().contains("curl -fsSL https://download.docker.com"));
    }

    #[test]
    fn test_github_spec_deserialization() {
        let yaml = r#"
repopath: custom_repos
"scottidler/aka":
  cargo:
    - ./
  link:
    "bin/aka.zsh": "~/.config/aka/aka.zsh"
    "bin/_aka_commands": "~/.shell-completions.d/_aka_commands"
  script:
    setup: |
      echo "Setting up aka"
      chmod +x bin/aka.zsh
"#;
        let spec: GithubSpec = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(spec.repopath, "custom_repos");
        assert_eq!(spec.items.len(), 1);

        let repo_spec = spec.items.get("scottidler/aka").unwrap();
        assert_eq!(repo_spec.cargo.len(), 1);
        assert_eq!(repo_spec.cargo[0], "./");
        assert_eq!(repo_spec.link.items.len(), 2);
        assert_eq!(repo_spec.link.items.get("bin/aka.zsh"), Some(&"~/.config/aka/aka.zsh".to_string()));
        assert_eq!(repo_spec.link.items.get("bin/_aka_commands"), Some(&"~/.shell-completions.d/_aka_commands".to_string()));
        assert_eq!(repo_spec.script.items.len(), 1);
        assert!(repo_spec.script.items.get("setup").unwrap().contains("Setting up aka"));
    }

    #[test]
    fn test_github_spec_default_repopath() {
        let yaml = r#"
"scottidler/test":
  cargo:
    - ./
"#;
        let spec: GithubSpec = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(spec.repopath, "repos"); // default value
    }

    #[test]
    fn test_repo_spec_deserialization() {
        let yaml = r#"
link:
  recursive: true
  "src": "dst"
cargo:
  - ./
  - subdir
script:
  build: |
    cargo build --release
  test: |
    cargo test
"#;
        let spec: RepoSpec = serde_yaml::from_str(yaml).unwrap();
        assert!(spec.link.recursive);
        assert_eq!(spec.link.items.len(), 1);
        assert_eq!(spec.link.items.get("src"), Some(&"dst".to_string()));
        assert_eq!(spec.cargo.len(), 2);
        assert!(spec.cargo.contains(&"./".to_string()));
        assert!(spec.cargo.contains(&"subdir".to_string()));
        assert_eq!(spec.script.items.len(), 2);
        assert!(spec.script.items.get("build").unwrap().contains("cargo build --release"));
        assert!(spec.script.items.get("test").unwrap().contains("cargo test"));
    }

    #[test]
    fn test_full_manifest_spec_deserialization() {
        let yaml = r#"
verbose: true
allow_errors: false

link:
  recursive: true
  HOME: $HOME

ppa:
  items:
    - git-core/ppa
    - mkusb/ppa

pkg:
  items:
    - jq
    - vim

apt:
  items:
    - fuse3
    - ldap-utils

dnf:
  items:
    - the_silver_searcher
    - gcc

npm:
  items:
    - diff-so-fancy
    - wt-cli

pip3:
  items:
    - argh
    - numpy
  distutils:
    - Cython
    - pexpect

pipx:
  items:
    - doit
    - mypy

flatpak:
  items:
    - org.gnome.GTG
    - org.gnome.BreakTimer

cargo:
  items:
    - bat
    - cargo-expand

github:
  repopath: repos
  "scottidler/test":
    cargo:
      - ./
    link:
      "bin/test": "~/bin/test"
    script:
      setup: |
        echo "Setting up test repo"

script:
  rust: |
    curl https://sh.rustup.rs -sSf | sh
  docker: |
    curl -fsSL https://download.docker.com/linux/ubuntu/gpg | sudo apt-key add -
"#;
        let spec: ManifestSpec = serde_yaml::from_str(yaml).unwrap();

        assert!(spec.verbose);
        assert!(!spec.errors);

        assert!(spec.link.recursive);
        assert_eq!(spec.link.items.len(), 1);

        assert_eq!(spec.ppa.items.len(), 2);
        assert_eq!(spec.pkg.items.len(), 2);
        assert_eq!(spec.apt.items.len(), 2);
        assert_eq!(spec.dnf.items.len(), 2);
        assert_eq!(spec.npm.items.len(), 2);

        assert_eq!(spec.pip3.items.len(), 2);
        assert_eq!(spec.pip3.distutils.len(), 2);

        assert_eq!(spec.pipx.items.len(), 2);
        assert_eq!(spec.flatpak.items.len(), 2);
        assert_eq!(spec.cargo.items.len(), 2);

        assert_eq!(spec.github.repopath, "repos");
        assert_eq!(spec.github.items.len(), 1);
        let repo_spec = spec.github.items.get("scottidler/test").unwrap();
        assert_eq!(repo_spec.cargo.len(), 1);
        assert_eq!(repo_spec.link.items.len(), 1);
        assert_eq!(repo_spec.script.items.len(), 1);

        assert_eq!(spec.script.items.len(), 2);
    }

    #[test]
    fn test_load_manifest_spec_from_actual_file() {
        // Test loading from the actual manifest.yml file
        let file = std::fs::File::open("manifest.yml").expect("manifest.yml should exist");
        let reader = std::io::BufReader::new(file);
        let spec = load_manifest_spec(reader).expect("Should parse manifest.yml successfully");

        // Verify some key properties from the actual manifest
        assert!(spec.verbose);
        assert!(!spec.errors);

        // Check that we have items in various sections
        assert!(!spec.ppa.items.is_empty());
        assert!(!spec.pkg.items.is_empty());
        assert!(!spec.apt.items.is_empty());
        assert!(!spec.dnf.items.is_empty());
        assert!(!spec.npm.items.is_empty());
        assert!(!spec.pip3.items.is_empty());
        assert!(!spec.pip3.distutils.is_empty());
        assert!(!spec.pipx.items.is_empty());
        assert!(!spec.flatpak.items.is_empty());
        assert!(!spec.cargo.items.is_empty());
        assert!(!spec.github.items.is_empty());
        assert!(!spec.script.items.is_empty());

        // Check specific items we know should be there
        assert!(spec.pkg.items.contains(&"jq".to_string()));
        assert!(spec.pkg.items.contains(&"vim".to_string()));
        assert!(spec.cargo.items.contains(&"bat".to_string()));
        assert!(spec.cargo.items.contains(&"ripgrep".to_string()));

        // Check that github repos are properly loaded
        assert!(spec.github.items.contains_key("scottidler/aka"));
        assert!(spec.github.items.contains_key("scottidler/nvim"));

        // Check that scripts are loaded
        assert!(spec.script.items.contains_key("rust"));
        assert!(spec.script.items.contains_key("docker"));
    }

    #[test]
    fn test_empty_specs_serialization() {
        let spec = ManifestSpec::default();
        let yaml = serde_yaml::to_string(&spec).unwrap();
        let deserialized: ManifestSpec = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(spec.verbose, deserialized.verbose);
        assert_eq!(spec.errors, deserialized.errors);
        assert_eq!(spec.link, deserialized.link);
        assert_eq!(spec.ppa.items, deserialized.ppa.items);
        assert_eq!(spec.pkg.items, deserialized.pkg.items);
    }

    #[test]
    fn test_repo_spec_with_nested_script() {
        // Test the case where a RepoSpec has a nested ScriptSpec
        let yaml = r#"
link:
  "bin/tool": "~/bin/tool"
cargo:
  - ./
script:
  post_install: |
    echo "Post-install script for repo"
    chmod +x ~/bin/tool
  configure: |
    echo "Configuring the tool"
    ~/bin/tool --setup
"#;
        let spec: RepoSpec = serde_yaml::from_str(yaml).unwrap();

        assert_eq!(spec.link.items.len(), 1);
        assert_eq!(spec.cargo.len(), 1);
        assert_eq!(spec.script.items.len(), 2);

        assert!(spec.script.items.contains_key("post_install"));
        assert!(spec.script.items.contains_key("configure"));

        let post_install = spec.script.items.get("post_install").unwrap();
        assert!(post_install.contains("Post-install script for repo"));
        assert!(post_install.contains("chmod +x ~/bin/tool"));

        let configure = spec.script.items.get("configure").unwrap();
        assert!(configure.contains("Configuring the tool"));
        assert!(configure.contains("~/bin/tool --setup"));
    }

    #[test]
    fn test_load_test_manifest_with_nested_scripts() {
        // Test loading from the test/manifest.yml file which includes nested ScriptSpec examples
        let file = std::fs::File::open("test/manifest.yml").expect("test/manifest.yml should exist");
        let reader = std::io::BufReader::new(file);
        let spec = load_manifest_spec(reader).expect("Should parse test/manifest.yml successfully");

        // Verify basic properties
        assert!(spec.verbose);
        assert!(!spec.errors);

        // Check that github section has repos with nested scripts
        assert_eq!(spec.github.repopath, "test_repos");
        assert!(spec.github.items.contains_key("testuser/tool-with-scripts"));
        assert!(spec.github.items.contains_key("testuser/simple-repo"));
        assert!(spec.github.items.contains_key("testuser/complex-repo"));

        // Test the repo with nested scripts
        let tool_repo = spec.github.items.get("testuser/tool-with-scripts").unwrap();
        assert_eq!(tool_repo.cargo.len(), 2);
        assert!(tool_repo.cargo.contains(&"./".to_string()));
        assert!(tool_repo.cargo.contains(&"cli-tool".to_string()));

        assert_eq!(tool_repo.link.items.len(), 3);
        assert_eq!(tool_repo.link.items.get("bin/tool"), Some(&"~/bin/tool".to_string()));
        assert_eq!(tool_repo.link.items.get("config/tool.conf"), Some(&"~/.config/tool/tool.conf".to_string()));
        assert_eq!(tool_repo.link.items.get("scripts/helper.sh"), Some(&"~/bin/helper.sh".to_string()));

        // Test nested scripts
        assert_eq!(tool_repo.script.items.len(), 3);
        assert!(tool_repo.script.items.contains_key("post_install"));
        assert!(tool_repo.script.items.contains_key("configure"));
        assert!(tool_repo.script.items.contains_key("test"));

        let post_install = tool_repo.script.items.get("post_install").unwrap();
        assert!(post_install.contains("Running post-install script for tool-with-scripts"));
        assert!(post_install.contains("chmod +x ~/bin/tool"));
        assert!(post_install.contains("mkdir -p ~/.config/tool"));

        let configure = tool_repo.script.items.get("configure").unwrap();
        assert!(configure.contains("Configuring tool-with-scripts"));
        assert!(configure.contains("~/bin/tool --init"));
        assert!(configure.contains("export PATH"));

        let test = tool_repo.script.items.get("test").unwrap();
        assert!(test.contains("Testing tool installation"));
        assert!(test.contains("~/bin/tool --version"));
        assert!(test.contains("~/bin/helper.sh --check"));

        // Test complex repo with multiple nested scripts
        let complex_repo = spec.github.items.get("testuser/complex-repo").unwrap();
        assert_eq!(complex_repo.cargo.len(), 3);
        assert!(complex_repo.cargo.contains(&"main-tool".to_string()));
        assert!(complex_repo.cargo.contains(&"sub-tool".to_string()));
        assert!(complex_repo.cargo.contains(&"utils".to_string()));

        assert!(complex_repo.link.recursive);
        assert_eq!(complex_repo.link.items.len(), 3);

        assert_eq!(complex_repo.script.items.len(), 3);
        assert!(complex_repo.script.items.contains_key("setup"));
        assert!(complex_repo.script.items.contains_key("build"));
        assert!(complex_repo.script.items.contains_key("post_build"));

        let setup = complex_repo.script.items.get("setup").unwrap();
        assert!(setup.contains("Setting up complex repo"));
        assert!(setup.contains("mkdir -p ~/.config/complex"));

        let build = complex_repo.script.items.get("build").unwrap();
        assert!(build.contains("Building all components"));
        assert!(build.contains("cargo build --release"));

        let post_build = complex_repo.script.items.get("post_build").unwrap();
        assert!(post_build.contains("Post-build configuration"));
        assert!(post_build.contains("cp target/release/main ~/bin/"));

        // Test simple repo without nested scripts
        let simple_repo = spec.github.items.get("testuser/simple-repo").unwrap();
        assert_eq!(simple_repo.cargo.len(), 1);
        assert_eq!(simple_repo.link.items.len(), 1);
        assert_eq!(simple_repo.script.items.len(), 0); // No nested scripts

        // Test top-level scripts
        assert_eq!(spec.script.items.len(), 4);
        assert!(spec.script.items.contains_key("rust"));
        assert!(spec.script.items.contains_key("docker"));
        assert!(spec.script.items.contains_key("nodejs"));
        assert!(spec.script.items.contains_key("development_tools"));

        let rust_script = spec.script.items.get("rust").unwrap();
        assert!(rust_script.contains("Installing Rust toolchain"));
        assert!(rust_script.contains("rustup component add clippy rustfmt"));

        let docker_script = spec.script.items.get("docker").unwrap();
        assert!(docker_script.contains("Installing Docker"));
        assert!(docker_script.contains("sudo usermod -aG docker $USER"));
    }
}

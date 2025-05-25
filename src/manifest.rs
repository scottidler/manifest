// src/manifest.rs

use std::collections::HashMap;
use crate::config::{RepoSpec, LinkSpec};

#[derive(Debug)]
pub enum ManifestType {
    Link(Vec<String>),
    Ppa(Vec<String>),
    Apt(Vec<String>),
    Dnf(Vec<String>),
    Npm(Vec<String>),
    Pip3(Vec<String>),
    Pipx(Vec<String>),
    Flatpak(Vec<String>),
    Cargo(Vec<String>),
    Github(HashMap<String, RepoSpec>),
    Script(HashMap<String, String>),
}

static LINKER: &str = include_str!("scripts/linker.sh");
static LATEST: &str = include_str!("scripts/latest.sh");

impl ManifestType {
    pub fn functions(&self) -> String {
        match self {
            ManifestType::Link(_) => LINKER.to_string(),
            ManifestType::Github(_) => LINKER.to_string(),
            ManifestType::Script(_) => LATEST.to_string(),
            _ => "".to_string(),
        }
    }

    pub fn render(&self) -> String {
        match self {
            ManifestType::Link(items) => {
                let header = r#"echo "links:""#;
                let block  = r#"linker $file $link"#;
                render_heredoc(header, block, items)
            }
            ManifestType::Ppa(items) => {
                let header = r#"echo "ppas:""#;
                let block  = r#"ppas=$(somecheck)
if [[ $ppas != *"$pkg"* ]]; then
  sudo add-apt-repository -y "ppa:$pkg"
fi"#;
                render_heredoc(header, block, items)
            }
            ManifestType::Apt(items) => {
                let header = r#"echo "apts:"
sudo apt update && sudo apt upgrade -y && sudo apt install -y software-properties-common"#;
                let block  = r#"sudo apt install -y"#;
                render_continue(header, block, items)
            }
            ManifestType::Dnf(items) => {
                let header = r#"echo "dnf packages:""#;
                let block  = r#"sudo dnf install -y"#;
                render_continue(header, block, items)
            }
            ManifestType::Npm(items) => {
                let header = r#"echo "npm packages:""#;
                let block  = r#"sudo npm install -g"#;
                render_continue(header, block, items)
            }
            ManifestType::Pip3(items) => {
                let header = r#"echo "pip3 packages:"
sudo apt-get install -y python3-dev
sudo -H pip3 install --upgrade pip setuptools"#;
                let block  = r#"sudo -H pip3 install --upgrade"#;
                render_continue(header, block, items)
            }
            ManifestType::Pipx(items) => {
                let header = r#"echo "pipx:""#;
                let block  = r#"pipx install "$pkg""#;
                render_heredoc(header, block, items)
            }
            ManifestType::Flatpak(items) => {
                let header = r#"echo "flatpaks:""#;
                let block  = r#"flatpak install --assumeyes --or-update"#;
                render_continue(header, block, items)
            }
            ManifestType::Cargo(items) => {
                let header = r#"echo "cargo crates:""#;
                let block  = r#"cargo install"#;
                render_continue(header, block, items)
            }
            ManifestType::Github(map) => {
                render_github(map, "repos")
            }
            ManifestType::Script(map) => render_script(map),
        }
    }
}

fn render_heredoc(header: &str, block: &str, items: &[String]) -> String {
    let items = items.join("\n");
    if header.is_empty() {
        format!(
"while read -r file link; do
    {block}
done<<EOM
{items}
EOM
", block = block, items = items)
    } else {
        format!(
r#"{header}

while read -r file link; do
    {block}
done<<EOM
{items}
EOM
"#,
            header = header,
            block = block,
            items = items
        )
    }
}

fn render_continue(header: &str, block: &str, items: &[String]) -> String {
    let items = items.join(" \\\n    ");
    format!(
r#"{header}

{block} {items}
"#,
        header = header,
        block = block,
        items = items
    )
}

fn render_repo_links(repo_path: &str, link_spec: &LinkSpec) -> String {
    if link_spec.items.is_empty() && !link_spec.recursive {
        return String::new();
    }

    let mut link_lines = Vec::new();
    for (src, dst) in &link_spec.items {
        let full_src = format!("{}/{}", repo_path, src);
        link_lines.push(format!("{} {}", full_src, dst));
    }

    let mut out = String::new();
    out.push_str("echo \"links:\"\n");
    out.push_str(&render_heredoc("", "linker $file $link", &link_lines));
    out.push('\n');
    out
}

/// Render per-repo `cargo install --path` section.
fn render_repo_cargo_install(repo_path: &str, paths: &[String]) -> String {
    if paths.is_empty() {
        return String::new();
    }

    let mut out = String::new();
    out.push_str("echo \"cargo install (path):\"\n");
    for rel_path in paths {
        let install_dir = format!("{}/{}", repo_path, rel_path);
        out.push_str(&format!("echo \"Installing from {}\"\n", install_dir));
        out.push_str(&format!(
            "(cd {} && cargo install --path .)\n",
            install_dir
        ));
    }
    out.push('\n');
    out
}

fn render_github(map: &HashMap<String, RepoSpec>, repopath: &str) -> String {
    let mut out = String::new();
    out.push_str("echo \"github repos:\"\n\n");

    for (repo_name, repo_spec) in map {
        let repo_path = format!("$HOME/{}/{}", repopath, repo_name);

        out.push_str(&format!("echo \"{}:\"\n", repo_name));
        out.push_str(&format!(
            "git clone --recursive https://github.com/{} {} \n",
            repo_name, repo_path
        ));
        out.push_str(&format!(
            "(cd {} && pwd && git pull && git checkout HEAD)\n",
            repo_path
        ));

        out.push_str(&render_repo_cargo_install(&repo_path, &repo_spec.cargo));

        out.push_str(&render_repo_links(&repo_path, &repo_spec.link));

        out.push_str(&render_script(&repo_spec.script.items));
        out.push('\n');

        out.push('\n');
    }

    out
}

fn render_script(map: &HashMap<String, String>) -> String {
    if map.is_empty() {
        return "".to_string();
    }
    let mut out = String::new();
    out.push_str("echo \"scripts:\"\n\n");
    for (name, body) in map {
        out.push_str(&format!("echo \"{}:\"\n", name));
        out.push_str(body);
        out.push('\n');
    }
    out
}

pub fn build_script(sections: &[ManifestType]) -> String {
    let mut script = String::new();
    script.push_str("#!/bin/bash\n");
    script.push_str("# generated file by manifest\n");
    script.push_str("# src: https://github.com/scottidler/manifest\n\n");
    script.push_str("if [ -n \"$DEBUG\" ]; then\n");
    script.push_str("    PS4=':${LINENO}+'\n");
    script.push_str("    set -x\n");
    script.push_str("fi\n\n");

    let mut blocks = Vec::new();
    for sec in sections {
        let f = sec.functions();
        if !f.trim().is_empty() && !blocks.contains(&f) {
            blocks.push(f);
        }
    }
    if !blocks.is_empty() {
        script.push_str(&blocks.join("\n\n"));
        script.push_str("\n\n");
    }

    for sec in sections {
        let body = sec.render();
        if !body.trim().is_empty() {
            script.push_str(&body);
            script.push('\n');
        }
    }
    script
}

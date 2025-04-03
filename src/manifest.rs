// src/manifest.rs

use std::collections::HashMap;

/// The single enum enumerating your "sections."
/// – The first nine variants store a Vec<String> (for Link, Ppa, Apt, Dnf, Npm, Pip3, Pipx, Flatpak, Cargo).
/// – The last two variants store a HashMap<String, String> (for Github, Script).
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
    Github(HashMap<String, String>),
    Script(HashMap<String, String>),
}

// Load the actual shell script files from disk.
static LINKER: &str = include_str!("scripts/linker.sh");
static LATEST: &str = include_str!("scripts/latest.sh");

impl ManifestType {
    /// Return any needed shell functions; deduplication will occur in build_script().
    pub fn functions(&self) -> String {
        match self {
            ManifestType::Link(_) => LINKER.to_string(),
            ManifestType::Github(_) => LINKER.to_string(),
            ManifestType::Script(_) => LATEST.to_string(),
            _ => "".to_string(),
        }
    }

    /// Return the final shell snippet for this variant.
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
            ManifestType::Github(map) => render_github(map),
            ManifestType::Script(map) => render_script(map),
        }
    }
}

/// Helper: renders a heredoc-style snippet.
/// Produces:
/// {header}
///
/// while read -r file link; do
///   {block}
/// done<<EOM
/// {items}
/// EOM
fn render_heredoc(header: &str, block: &str, items: &[String]) -> String {
    let items = items.join("\n");
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

/// Helper: renders a continuation-style snippet.
/// Produces a header followed by a command line where the items are joined
/// with a " \\\n    " separator, which results in a shell command that spans
/// multiple indented lines. For example:
///
/// echo "apts:"
///
/// sudo apt install -y item1 \
///     item2 \
///     item3
fn render_continue(header: &str, block: &str, items: &[String]) -> String {
    // Join items with a backslash and a newline for readability.
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

/// Helper: renders the GitHub variant (custom logic).
fn render_github(map: &HashMap<String, String>) -> String {
    let mut out = String::new();
    out.push_str(r#"echo "github repos:""#);
    out.push('\n');
    for (k, v) in map {
        out.push_str(&format!("echo \"Repo {k}: {v}\"\n"));
    }
    out
}

/// Helper: renders the Script variant (custom logic).
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

/// Aggregates all sections into the final script. It deduplicates shell function blocks,
/// then appends each section’s render() output.
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

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
    Github(HashMap<String, RepoSpec>, String),
    GitCrypt(HashMap<String, RepoSpec>, String),
    Script(HashMap<String, String>),
}

static LINKER: &str = include_str!("scripts/linker.sh");
static LATEST: &str = include_str!("scripts/latest.sh");

impl ManifestType {
    pub fn functions(&self) -> String {
        match self {
            ManifestType::Link(_) => LINKER.to_string(),
            ManifestType::Github(_, _) => LINKER.to_string(),
            ManifestType::GitCrypt(_, _) => LINKER.to_string(),
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
            ManifestType::Github(map, repopath) => {
                render_github(map, repopath)
            }
            ManifestType::GitCrypt(map, repopath) => {
                render_gitcrypt(map, repopath)
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
r#"
{header}
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
r#"
{header}
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
        let path = std::path::Path::new(repo_path).join(src);
        let full_src = path.components()
            .filter(|c| *c != std::path::Component::CurDir)
            .collect::<std::path::PathBuf>()
            .to_string_lossy()
            .to_string();
        link_lines.push(format!("{} {}", full_src, dst));
    }

    let mut out = String::new();
    out.push_str("echo \"links:\"\n");
    out.push_str(&render_heredoc("", "linker $file $link", &link_lines));
    out
}

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
    out
}

fn render_github(map: &HashMap<String, RepoSpec>, repopath: &str) -> String {
    let mut out = String::new();
    out.push_str("\necho \"github repos:\"\n");

    let repos: Vec<_> = map.iter().collect();
    for (i, (repo_name, repo_spec)) in repos.iter().enumerate() {
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

        // Add blank line between repos for readability, but not after the last one
        if i < repos.len() - 1 {
            out.push('\n');
        }
    }

    out
}

fn render_gitcrypt(map: &HashMap<String, RepoSpec>, repopath: &str) -> String {
    let mut out = String::new();
    out.push_str("\necho \"git-crypt repos:\"\n");

    // Check for git-crypt binary
    out.push_str("if ! hash git-crypt >/dev/null 2>&1; then\n");
    out.push_str("  echo \"Error: git-crypt not found. Install with: apt install git-crypt\"\n");
    out.push_str("  exit 1\n");
    out.push_str("fi\n\n");

    // Check for environment variable
    out.push_str("if [[ -z \"${GIT_CRYPT_PASSWORD}\" ]]; then\n");
    out.push_str("  echo \"Error: GIT_CRYPT_PASSWORD environment variable not set\"\n");
    out.push_str("  echo \"Set it with: export GIT_CRYPT_PASSWORD='your-passphrase'\"\n");
    out.push_str("  exit 1\n");
    out.push_str("fi\n\n");

    let repos: Vec<_> = map.iter().collect();
    for (i, (repo_name, repo_spec)) in repos.iter().enumerate() {
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

        // git-crypt unlock step
        out.push_str(&format!(
            "if ! (cd {} && echo \"$GIT_CRYPT_PASSWORD\" | git-crypt unlock -); then\n",
            repo_path
        ));
        out.push_str(&format!(
            "  echo \"Error: Failed to unlock git-crypt repo {}\"\n",
            repo_name
        ));
        out.push_str("  exit 1\n");
        out.push_str("fi\n");

        out.push_str(&render_repo_cargo_install(&repo_path, &repo_spec.cargo));
        out.push_str(&render_repo_links(&repo_path, &repo_spec.link));
        out.push_str(&render_script(&repo_spec.script.items));

        // Add blank line between repos for readability, but not after the last one
        if i < repos.len() - 1 {
            out.push('\n');
        }
    }

    out
}

fn render_script(map: &HashMap<String, String>) -> String {
    if map.is_empty() {
        return "".to_string();
    }
    let mut out = String::new();
    out.push_str("echo \"scripts:\"\n");
    let scripts: Vec<_> = map.iter().collect();
    for (i, (name, body)) in scripts.iter().enumerate() {
        out.push_str(&format!("echo \"{}:\"\n", name));
        out.push_str(body);
        if i < scripts.len() - 1 {
            out.push('\n');
        }
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
        script.push_str(&blocks.join("\n"));
        script.push_str("\n");
    }

    for (i, sec) in sections.iter().enumerate() {
        let mut body = sec.render();
        if !body.trim().is_empty() {
            if i == 0 && body.starts_with('\n') {
                body = body[1..].to_string();
            }
            script.push_str(&body);
        }
    }
    script
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_manifest_type_link_render() {
        let items = vec![
            "src1 dst1".to_string(),
            "src2 dst2".to_string(),
        ];
        let manifest_type = ManifestType::Link(items);
        let rendered = manifest_type.render();

        assert!(rendered.contains("echo \"links:\""));
        assert!(rendered.contains("while read -r file link; do"));
        assert!(rendered.contains("linker $file $link"));
        assert!(rendered.contains("src1 dst1"));
        assert!(rendered.contains("src2 dst2"));
        assert!(rendered.contains("done<<EOM"));
    }

    #[test]
    fn test_manifest_type_ppa_render() {
        let items = vec![
            "git-core/ppa".to_string(),
            "mkusb/ppa".to_string(),
        ];
        let manifest_type = ManifestType::Ppa(items);
        let rendered = manifest_type.render();

        assert!(rendered.contains("echo \"ppas:\""));
        assert!(rendered.contains("while read -r file link; do"));
        assert!(rendered.contains("ppas=$(somecheck)"));
        assert!(rendered.contains("if [[ $ppas != *\"$pkg\"* ]]; then"));
        assert!(rendered.contains("sudo add-apt-repository -y \"ppa:$pkg\""));
        assert!(rendered.contains("git-core/ppa"));
        assert!(rendered.contains("mkusb/ppa"));
    }

    #[test]
    fn test_manifest_type_apt_render() {
        let items = vec![
            "fuse3".to_string(),
            "ldap-utils".to_string(),
            "fonts-powerline".to_string(),
        ];
        let manifest_type = ManifestType::Apt(items);
        let rendered = manifest_type.render();

        assert!(rendered.contains("echo \"apts:\""));
        assert!(rendered.contains("sudo apt update && sudo apt upgrade -y"));
        assert!(rendered.contains("sudo apt install -y software-properties-common"));
        assert!(rendered.contains("sudo apt install -y fuse3 \\"));
        assert!(rendered.contains("ldap-utils \\"));
        assert!(rendered.contains("fonts-powerline"));
    }

    #[test]
    fn test_manifest_type_dnf_render() {
        let items = vec![
            "the_silver_searcher".to_string(),
            "gcc".to_string(),
        ];
        let manifest_type = ManifestType::Dnf(items);
        let rendered = manifest_type.render();

        assert!(rendered.contains("echo \"dnf packages:\""));
        assert!(rendered.contains("sudo dnf install -y the_silver_searcher \\"));
        assert!(rendered.contains("gcc"));
    }

    #[test]
    fn test_manifest_type_npm_render() {
        let items = vec![
            "diff-so-fancy".to_string(),
            "wt-cli".to_string(),
        ];
        let manifest_type = ManifestType::Npm(items);
        let rendered = manifest_type.render();

        assert!(rendered.contains("echo \"npm packages:\""));
        assert!(rendered.contains("sudo npm install -g diff-so-fancy \\"));
        assert!(rendered.contains("wt-cli"));
    }

    #[test]
    fn test_manifest_type_pip3_render() {
        let items = vec![
            "argh".to_string(),
            "numpy".to_string(),
            "twine".to_string(),
        ];
        let manifest_type = ManifestType::Pip3(items);
        let rendered = manifest_type.render();

        assert!(rendered.contains("echo \"pip3 packages:\""));
        assert!(rendered.contains("sudo apt-get install -y python3-dev"));
        assert!(rendered.contains("sudo -H pip3 install --upgrade pip setuptools"));
        assert!(rendered.contains("sudo -H pip3 install --upgrade argh \\"));
        assert!(rendered.contains("numpy \\"));
        assert!(rendered.contains("twine"));
    }

    #[test]
    fn test_manifest_type_pipx_render() {
        let items = vec![
            "doit".to_string(),
            "mypy".to_string(),
        ];
        let manifest_type = ManifestType::Pipx(items);
        let rendered = manifest_type.render();

        assert!(rendered.contains("echo \"pipx:\""));
        assert!(rendered.contains("while read -r file link; do"));
        assert!(rendered.contains("pipx install \"$pkg\""));
        assert!(rendered.contains("doit"));
        assert!(rendered.contains("mypy"));
    }

    #[test]
    fn test_manifest_type_flatpak_render() {
        let items = vec![
            "org.gnome.GTG".to_string(),
            "org.gnome.BreakTimer".to_string(),
        ];
        let manifest_type = ManifestType::Flatpak(items);
        let rendered = manifest_type.render();

        assert!(rendered.contains("echo \"flatpaks:\""));
        assert!(rendered.contains("flatpak install --assumeyes --or-update org.gnome.GTG \\"));
        assert!(rendered.contains("org.gnome.BreakTimer"));
    }

    #[test]
    fn test_manifest_type_cargo_render() {
        let items = vec![
            "bat".to_string(),
            "cargo-expand".to_string(),
        ];
        let manifest_type = ManifestType::Cargo(items);
        let rendered = manifest_type.render();

        assert!(rendered.contains("echo \"cargo crates:\""));
        assert!(rendered.contains("cargo install bat \\"));
        assert!(rendered.contains("cargo-expand"));
    }

    #[test]
    fn test_manifest_type_script_render() {
        let mut items = HashMap::new();
        items.insert("rust".to_string(), "curl https://sh.rustup.rs -sSf | sh".to_string());
        items.insert("docker".to_string(), "curl -fsSL https://download.docker.com/linux/ubuntu/gpg | sudo apt-key add -\nsudo add-apt-repository \"deb [arch=amd64] https://download.docker.com/linux/ubuntu $(lsb_release -cs) stable\"".to_string());

        let manifest_type = ManifestType::Script(items);
        let rendered = manifest_type.render();

        assert!(rendered.contains("echo \"scripts:\""));
        assert!(rendered.contains("echo \"rust:\"") || rendered.contains("echo \"docker:\""));
        assert!(rendered.contains("curl https://sh.rustup.rs -sSf | sh"));
        assert!(rendered.contains("curl -fsSL https://download.docker.com/linux/ubuntu/gpg"));
    }

    #[test]
    fn test_manifest_type_github_render() {
        let mut items = HashMap::new();
        let mut repo_spec = crate::config::RepoSpec::default();
        repo_spec.cargo = vec!["./".to_string()];
        repo_spec.link.items.insert("bin/test".to_string(), "~/bin/test".to_string());
        repo_spec.script.items.insert("setup".to_string(), "echo 'Setting up test repo'".to_string());
        items.insert("scottidler/test".to_string(), repo_spec);

        let manifest_type = ManifestType::Github(items, "repos".to_string());
        let rendered = manifest_type.render();

        assert!(rendered.contains("echo \"github repos:\""));
        assert!(rendered.contains("echo \"scottidler/test:\""));
        assert!(rendered.contains("git clone --recursive https://github.com/scottidler/test"));
        assert!(rendered.contains("cargo install --path"));
        assert!(rendered.contains("linker"));
        assert!(rendered.contains("echo \"setup:\""));
        assert!(rendered.contains("Setting up test repo"));
    }

    #[test]
    fn test_manifest_type_git_crypt_render() {
        let mut items = HashMap::new();
        let mut repo_spec = crate::config::RepoSpec::default();
        repo_spec.link.items.insert("ssh/id_rsa".to_string(), "~/.ssh/id_rsa".to_string());
        repo_spec.link.items.insert("gpg/private.asc".to_string(), "~/.gnupg/private.asc".to_string());
        repo_spec.script.items.insert("post_unlock".to_string(), "chmod 600 ~/.ssh/id_rsa\ngpg --import ~/.gnupg/private.asc".to_string());
        items.insert("scottidler/secrets".to_string(), repo_spec);

        let manifest_type = ManifestType::GitCrypt(items, "repos".to_string());
        let rendered = manifest_type.render();

        assert!(rendered.contains("echo \"git-crypt repos:\""));
        assert!(rendered.contains("if ! hash git-crypt >/dev/null 2>&1; then"));
        assert!(rendered.contains("Error: git-crypt not found"));
        assert!(rendered.contains("if [[ -z \"${GIT_CRYPT_PASSWORD}\" ]]; then"));
        assert!(rendered.contains("Error: GIT_CRYPT_PASSWORD environment variable not set"));
        assert!(rendered.contains("echo \"scottidler/secrets:\""));
        assert!(rendered.contains("git clone --recursive https://github.com/scottidler/secrets"));
        assert!(rendered.contains("echo \"$GIT_CRYPT_PASSWORD\" | git-crypt unlock -"));
        assert!(rendered.contains("linker"));
        assert!(rendered.contains("~/.ssh/id_rsa"));
        assert!(rendered.contains("~/.gnupg/private.asc"));
        assert!(rendered.contains("chmod 600 ~/.ssh/id_rsa"));
        assert!(rendered.contains("gpg --import ~/.gnupg/private.asc"));
    }

    #[test]
    fn test_manifest_type_functions() {
        let link_type = ManifestType::Link(vec![]);
        assert_eq!(link_type.functions(), LINKER);

        let github_type = ManifestType::Github(HashMap::new(), "repos".to_string());
        assert_eq!(github_type.functions(), LINKER);

        let gitcrypt_type = ManifestType::GitCrypt(HashMap::new(), "repos".to_string());
        assert_eq!(gitcrypt_type.functions(), LINKER);

        let script_type = ManifestType::Script(HashMap::new());
        assert_eq!(script_type.functions(), LATEST);

        let apt_type = ManifestType::Apt(vec![]);
        assert_eq!(apt_type.functions(), "");

        let cargo_type = ManifestType::Cargo(vec![]);
        assert_eq!(cargo_type.functions(), "");
    }

    #[test]
    fn test_render_heredoc() {
        let header = "echo \"test:\"";
        let block = "echo $file $link";
        let items = vec!["item1 value1".to_string(), "item2 value2".to_string()];

        let result = render_heredoc(header, block, &items);

        assert!(result.contains("echo \"test:\""));
        assert!(result.contains("while read -r file link; do"));
        assert!(result.contains("echo $file $link"));
        assert!(result.contains("done<<EOM"));
        assert!(result.contains("item1 value1"));
        assert!(result.contains("item2 value2"));
        assert!(result.contains("EOM"));
    }

    #[test]
    fn test_render_heredoc_empty_header() {
        let header = "";
        let block = "echo $file $link";
        let items = vec!["item1 value1".to_string()];

        let result = render_heredoc(header, block, &items);

        assert!(!result.contains("echo \"test:\""));
        assert!(result.contains("while read -r file link; do"));
        assert!(result.contains("echo $file $link"));
        assert!(result.contains("item1 value1"));
    }

    #[test]
    fn test_render_continue() {
        let header = "echo \"packages:\"";
        let block = "sudo apt install -y";
        let items = vec!["package1".to_string(), "package2".to_string(), "package3".to_string()];

        let result = render_continue(header, block, &items);

        assert!(result.contains("echo \"packages:\""));
        assert!(result.contains("sudo apt install -y package1 \\"));
        assert!(result.contains("package2 \\"));
        assert!(result.contains("package3"));
        assert!(!result.contains("package3 \\"));
    }

    #[test]
    fn test_render_repo_links() {
        let mut link_spec = crate::config::LinkSpec::default();
        link_spec.items.insert("bin/tool".to_string(), "~/bin/tool".to_string());
        link_spec.items.insert("config/tool.conf".to_string(), "~/.config/tool.conf".to_string());

        let result = render_repo_links("$HOME/repos/test", &link_spec);

        assert!(result.contains("echo \"links:\""));
        assert!(result.contains("$HOME/repos/test/bin/tool ~/bin/tool"));
        assert!(result.contains("$HOME/repos/test/config/tool.conf ~/.config/tool.conf"));
        assert!(result.contains("linker $file $link"));
    }

    #[test]
    fn test_render_repo_links_empty() {
        let link_spec = crate::config::LinkSpec::default();
        let result = render_repo_links("$HOME/repos/test", &link_spec);
        assert!(result.is_empty());
    }

    #[test]
    fn test_render_repo_cargo_install() {
        let paths = vec!["./".to_string(), "subdir".to_string()];
        let result = render_repo_cargo_install("$HOME/repos/test", &paths);

        assert!(result.contains("echo \"cargo install (path):\""));
        assert!(result.contains("echo \"Installing from $HOME/repos/test/./\""));
        assert!(result.contains("(cd $HOME/repos/test/./ && cargo install --path .)"));
        assert!(result.contains("echo \"Installing from $HOME/repos/test/subdir\""));
        assert!(result.contains("(cd $HOME/repos/test/subdir && cargo install --path .)"));
    }

    #[test]
    fn test_render_repo_cargo_install_empty() {
        let paths = vec![];
        let result = render_repo_cargo_install("$HOME/repos/test", &paths);
        assert!(result.is_empty());
    }

    #[test]
    fn test_render_github() {
        let mut items = HashMap::new();
        let mut repo_spec1 = crate::config::RepoSpec::default();
        repo_spec1.cargo = vec!["./".to_string()];
        repo_spec1.link.items.insert("bin/tool1".to_string(), "~/bin/tool1".to_string());
        repo_spec1.script.items.insert("setup".to_string(), "echo 'Setting up tool1'".to_string());

        let mut repo_spec2 = crate::config::RepoSpec::default();
        repo_spec2.cargo = vec!["subdir".to_string()];
        repo_spec2.link.items.insert("bin/tool2".to_string(), "~/bin/tool2".to_string());

        items.insert("user/repo1".to_string(), repo_spec1);
        items.insert("user/repo2".to_string(), repo_spec2);

        let result = render_github(&items, "repos");

        assert!(result.contains("echo \"github repos:\""));
        assert!(result.contains("echo \"user/repo1:\"") || result.contains("echo \"user/repo2:\""));
        assert!(result.contains("git clone --recursive https://github.com/user/repo1"));
        assert!(result.contains("git clone --recursive https://github.com/user/repo2"));
        assert!(result.contains("cargo install --path"));
        assert!(result.contains("linker"));
        assert!(result.contains("Setting up tool1"));
    }

    #[test]
    fn test_render_script() {
        let mut items = HashMap::new();
        items.insert("script1".to_string(), "echo 'Running script1'\necho 'Done'".to_string());
        items.insert("script2".to_string(), "echo 'Running script2'".to_string());

        let result = render_script(&items);

        assert!(result.contains("echo \"scripts:\""));
        assert!(result.contains("echo \"script1:\"") || result.contains("echo \"script2:\""));
        assert!(result.contains("Running script1") || result.contains("Running script2"));
    }

    #[test]
    fn test_render_script_empty() {
        let items = HashMap::new();
        let result = render_script(&items);
        assert!(result.is_empty());
    }

    #[test]
    fn test_build_script_empty() {
        let sections = vec![];
        let result = build_script(&sections);

        assert!(result.contains("#!/bin/bash"));
        assert!(result.contains("# generated file by manifest"));
        assert!(result.contains("# src: https://github.com/scottidler/manifest"));
        assert!(result.contains("if [ -n \"$DEBUG\" ]; then"));
        assert!(result.contains("PS4=':${LINENO}+'"));
        assert!(result.contains("set -x"));
        assert!(result.contains("fi"));
    }

    #[test]
    fn test_build_script_with_functions() {
        let sections = vec![
            ManifestType::Link(vec!["src dst".to_string()]),
            ManifestType::Script(HashMap::from([("test".to_string(), "echo test".to_string())])),
        ];
        let result = build_script(&sections);

        assert!(result.contains("#!/bin/bash"));
        assert!(result.contains("# generated file by manifest"));

        assert!(result.contains("linker() {"));
        assert!(result.contains("latest() {"));

        assert!(result.contains("echo \"links:\""));
        assert!(result.contains("echo \"scripts:\""));
    }

    #[test]
    fn test_build_script_deduplicates_functions() {
        let sections = vec![
            ManifestType::Link(vec!["src1 dst1".to_string()]),
            ManifestType::Github(HashMap::new(), "repos".to_string()),
            ManifestType::Link(vec!["src2 dst2".to_string()]),
        ];
        let result = build_script(&sections);

        let linker_count = result.matches("linker() {").count();
        assert_eq!(linker_count, 1);
    }

    #[test]
    fn test_build_script_removes_leading_newline_from_first_section() {
        let sections = vec![
            ManifestType::Apt(vec!["package1".to_string()]),
        ];
        let result = build_script(&sections);

        let lines: Vec<&str> = result.lines().collect();
        let debug_end_idx = lines.iter().position(|&line| line == "fi").unwrap();

        assert!(lines[debug_end_idx + 2].contains("echo \"apts:\""));
    }

    #[test]
    fn test_integration_with_repo_nested_scripts() {
        let mut repo_spec = crate::config::RepoSpec::default();
        repo_spec.script.items.insert("post_install".to_string(), "echo 'Post install script'\nchmod +x ~/bin/tool".to_string());
        repo_spec.script.items.insert("configure".to_string(), "echo 'Configuration script'\n~/bin/tool --setup".to_string());

        let mut github_items = HashMap::new();
        github_items.insert("user/repo".to_string(), repo_spec);

        let github_type = ManifestType::Github(github_items, "repos".to_string());
        let rendered = github_type.render();

        assert!(rendered.contains("echo \"scripts:\""));
        assert!(rendered.contains("echo \"post_install:\"") || rendered.contains("echo \"configure:\""));
        assert!(rendered.contains("Post install script") || rendered.contains("Configuration script"));
        assert!(rendered.contains("chmod +x ~/bin/tool") || rendered.contains("~/bin/tool --setup"));
    }
}

// src/main.rs

mod cli;
mod config;

use clap::Parser;
use crate::cli::Cli;
use crate::config::{
    filter_by_patterns, recursive_links,
    Manifest, ManifestSpec, LinkSpec, PpaSpec, PackageSpec, GithubRepoSpec
};
use eyre::{Result, eyre};
use log::info;
use std::process::Command;

fn main() -> Result<()> {
    env_logger::init(); // optional logging
    let args = Cli::parse();
    info!("CLI args: {:?}", args);

    // Load the manifest
    let manifest = Manifest::load_from_file(&args.config)?;
    info!("Loaded manifest: {:?}", manifest.spec);

    // If the user did not specify ANY of the multi-value flags, we do "complete" mode
    let is_partial = is_partial_selection(&args);
    let script = generate_bash_script(&manifest.spec, &args, !is_partial)?;
    println!("{}", script);

    Ok(())
}

/// If user typed none of the multi-value flags, we do "complete" (like the Python code).
/// If user typed ANY of them, we do partial.
fn is_partial_selection(args: &Cli) -> bool {
    // If any of these vectors is non-empty, it means the user typed that flag (since 
    // omit => empty, typed no args => ["*"], typed args => that set).
    // But note that if it's exactly [] because user omitted the flag, that's different from ["*"].
    !args.link.is_empty()
        || !args.ppa.is_empty()
        || !args.apt.is_empty()
        || !args.dnf.is_empty()
        || !args.npm.is_empty()
        || !args.pip3.is_empty()
        || !args.pipx.is_empty()
        || !args.flatpak.is_empty()
        || !args.cargo.is_empty()
        || !args.github.is_empty()
        || !args.script.is_empty()
}

/// Build the final Bash script
fn generate_bash_script(spec: &ManifestSpec, args: &Cli, include_all: bool) -> Result<String> {
    // Use 'verbose' / 'errors' so they're not “dead code”
    if spec.verbose {
        eprintln!("(Verbose mode on, but no special logic here.)");
    }
    if spec.errors {
        eprintln!("(Errors mode on, but no special logic here.)");
    }

    let mut bash = String::new();

    // 1. Header
    bash.push_str("#!/bin/bash\n");
    bash.push_str("# generated file by manifest-rs\n\n");

    // 2. DEBUG snippet
    bash.push_str(r#"if [ -n "$DEBUG" ]; then
    PS4=':${LINENO}+'
    set -x
fi

"#);

    // 3. Add the two “utility” functions
    bash.push_str(include_linker_fn().as_str());
    bash.push_str("\n");
    bash.push_str(include_latest_fn().as_str());
    bash.push_str("\n");

    // 4. Determine pkgmgr if not provided
    let pkgmgr = match detect_pkgmgr(&args.pkgmgr) {
        Ok(pm) => pm,
        Err(e) => {
            eprintln!("Could not auto-detect pkgmgr: {e}, defaulting to deb");
            "deb".to_string()
        }
    };

    // 5. If “complete” or user specifically asked for link
    if (include_all || !args.link.is_empty()) && spec.link.is_some() {
        bash.push_str(&render_link(spec.link.as_ref().unwrap(), &args.link, args)?);
        bash.push_str("\n");
    }

    // ppa
    if (include_all || !args.ppa.is_empty()) && spec.ppa.is_some() {
        bash.push_str(&render_ppa(spec.ppa.as_ref().unwrap(), &args.ppa)?);
        bash.push_str("\n");
    }

    // Merge pkg with apt/dnf
    match pkgmgr.as_str() {
        "deb" => {
            let pkgs = merged_packages(spec.pkg.as_ref(), spec.apt.as_ref());
            if (include_all || !args.apt.is_empty()) && !pkgs.is_empty() {
                bash.push_str(&render_apt(&pkgs, &args.apt)?);
                bash.push_str("\n");
            }
        },
        "rpm" => {
            let pkgs = merged_packages(spec.pkg.as_ref(), spec.dnf.as_ref());
            if (include_all || !args.dnf.is_empty()) && !pkgs.is_empty() {
                bash.push_str(&render_dnf(&pkgs, &args.dnf)?);
                bash.push_str("\n");
            }
        },
        "brew" => {
            // ...
        },
        _ => {
            // fallback
        }
    }

    // npm
    if (include_all || !args.npm.is_empty()) && spec.npm.is_some() {
        bash.push_str(&render_npm(spec.npm.as_ref().unwrap(), &args.npm)?);
        bash.push_str("\n");
    }

    // pip3
    if (include_all || !args.pip3.is_empty()) && spec.pip3.is_some() {
        bash.push_str(&render_pip3(spec.pip3.as_ref().unwrap(), &args.pip3)?);
        bash.push_str("\n");
    }

    // pipx
    if (include_all || !args.pipx.is_empty()) && spec.pipx.is_some() {
        bash.push_str(&render_pipx(spec.pipx.as_ref().unwrap(), &args.pipx)?);
        bash.push_str("\n");
    }

    // flatpak
    if (include_all || !args.flatpak.is_empty()) && spec.flatpak.is_some() {
        bash.push_str(&render_flatpak(spec.flatpak.as_ref().unwrap(), &args.flatpak)?);
        bash.push_str("\n");
    }

    // cargo
    if (include_all || !args.cargo.is_empty()) && spec.cargo.is_some() {
        bash.push_str(&render_cargo(spec.cargo.as_ref().unwrap(), &args.cargo)?);
        bash.push_str("\n");
    }

    // github
    if (include_all || !args.github.is_empty()) && !spec.github.is_empty() {
        bash.push_str(&render_github(&spec.github, &args.github, args)?);
        bash.push_str("\n");
    }

    // script
    if (include_all || !args.script.is_empty()) && !spec.script.is_empty() {
        bash.push_str(&render_script(&spec.script, &args.script)?);
        bash.push_str("\n");
    }

    Ok(bash)
}

fn detect_pkgmgr(explicit: &str) -> Result<String> {
    if !explicit.is_empty() {
        return Ok(explicit.to_string());
    }
    if hash_exists("dpkg") {
        Ok("deb".to_string())
    } else if hash_exists("rpm") {
        Ok("rpm".to_string())
    } else if hash_exists("brew") {
        Ok("brew".to_string())
    } else {
        Err(eyre!("no known pkgmgr found"))
    }
}

/// Check if `hash <cmd>` works
fn hash_exists(cmd: &str) -> bool {
    match Command::new("bash").arg("-c").arg(format!("hash {}", cmd)).status() {
        Ok(s) => s.success(),
        Err(_) => false,
    }
}

/// Merges "pkg" with apt/dnf items
fn merged_packages(pkg: Option<&PackageSpec>, distro: Option<&PackageSpec>) -> Vec<String> {
    let mut merged = Vec::new();
    if let Some(p) = pkg {
        if let Some(ref items) = p.items {
            merged.extend(items.clone());
        }
    }
    if let Some(d) = distro {
        if let Some(ref items) = d.items {
            merged.extend(items.clone());
        }
    }
    merged
}

/// Renders the “link” section
fn render_link(spec: &LinkSpec, patterns: &[String], args: &Cli) -> Result<String> {
    let mut bash = String::new();
    bash.push_str("echo \"links:\"\n");

    let mut lines = Vec::new();
    let recursive = spec.recursive.unwrap_or(false);

    for (srcpath, dstpath) in &spec.items {
        // If the user’s patterns == ["*"], accept all. Otherwise we filter.
        if patterns != &["*".to_string()] {
            if !matches_any_pattern(srcpath, patterns) {
                continue;
            }
        }
        if recursive {
            let rec = recursive_links(srcpath, dstpath, &args.cwd, &args.home);
            lines.extend(rec);
        } else {
            let replaced = dstpath.replace("~", &args.home).replace("$HOME", &args.home);
            let source_abs = format!("{}/{}", args.cwd, srcpath);
            lines.push((source_abs, replaced));
        }
    }

    if lines.is_empty() {
        return Ok(bash);
    }

    bash.push_str("while read -r file link; do\n    linker \"$file\" \"$link\"\ndone<<EOM\n");
    for (file, link) in lines {
        bash.push_str(&format!("{} {}\n", file, link));
    }
    bash.push_str("EOM\n");

    Ok(bash)
}

/// Helper for pattern matching
fn matches_any_pattern(s: &str, patterns: &[String]) -> bool {
    for pat in patterns {
        if pat == "*" {
            return true;
        }
        if let Ok(g) = glob::Pattern::new(pat) {
            if g.matches(s) {
                return true;
            }
        }
    }
    false
}

/// Renders the PPA section
fn render_ppa(spec: &PpaSpec, patterns: &[String]) -> Result<String> {
    let mut bash = String::new();
    bash.push_str("echo \"ppa:\" \n");

    if let Some(ref items) = spec.items {
        let matched = filter_by_patterns(items, patterns);
        if matched.is_empty() {
            return Ok(bash);
        }
        bash.push_str("while read pkg; do\n");
        bash.push_str("    ppas=$(find /etc/apt/ -name '*.list' | xargs cat | grep -E '^\\s*deb' | grep -v deb-src)\n");
        bash.push_str("    if [[ $ppas != *\"$pkg\"* ]]; then\n");
        bash.push_str("        sudo add-apt-repository -y \"ppa:$pkg\"\n");
        bash.push_str("    fi\n");
        bash.push_str("done<<EOM\n");
        for p in matched {
            bash.push_str(&format!("{}\n", p));
        }
        bash.push_str("EOM\n");
    }
    Ok(bash)
}

/// APT
fn render_apt(pkgs: &[String], patterns: &[String]) -> Result<String> {
    let matched = filter_by_patterns(pkgs, patterns);
    if matched.is_empty() {
        return Ok(String::new());
    }
    let mut bash = String::new();
    bash.push_str("echo \"apt packages:\" \n");
    bash.push_str("sudo apt update && sudo apt upgrade -y && sudo apt install -y software-properties-common\n");
    bash.push_str("sudo apt install -y \\\n");
    for (i, m) in matched.iter().enumerate() {
        if i == matched.len() - 1 {
            bash.push_str(&format!("    {}\n", m));
        } else {
            bash.push_str(&format!("    {} \\\n", m));
        }
    }
    Ok(bash)
}

/// DNF
fn render_dnf(pkgs: &[String], patterns: &[String]) -> Result<String> {
    let matched = filter_by_patterns(pkgs, patterns);
    if matched.is_empty() {
        return Ok(String::new());
    }
    let mut bash = String::new();
    bash.push_str("echo \"dnf packages:\" \n");
    bash.push_str("for pkg in \\\n");
    for (i, m) in matched.iter().enumerate() {
        if i == matched.len() - 1 {
            bash.push_str(&format!("    {}\n", m));
        } else {
            bash.push_str(&format!("    {} \\\n", m));
        }
    }
    bash.push_str("; do\n");
    bash.push_str("    sudo dnf install -y $pkg\n");
    bash.push_str("done\n");
    Ok(bash)
}

/// npm
fn render_npm(spec: &PackageSpec, patterns: &[String]) -> Result<String> {
    let mut bash = String::new();
    bash.push_str("echo \"npm packages:\" \n");
    if let Some(ref items) = spec.items {
        let matched = filter_by_patterns(items, patterns);
        if matched.is_empty() {
            return Ok(bash);
        }
        bash.push_str("sudo npm install -g \\\n");
        for (i, m) in matched.iter().enumerate() {
            if i == matched.len() - 1 {
                bash.push_str(&format!("    {}\n", m));
            } else {
                bash.push_str(&format!("    {} \\\n", m));
            }
        }
    }
    Ok(bash)
}

/// pip3
fn render_pip3(spec: &PackageSpec, patterns: &[String]) -> Result<String> {
    let mut bash = String::new();
    bash.push_str("echo \"pip3 packages:\" \n");
    bash.push_str("sudo apt-get install -y python3-dev\n");
    bash.push_str("sudo -H pip3 install --upgrade pip setuptools\n");
    if let Some(ref items) = spec.items {
        let matched = filter_by_patterns(items, patterns);
        if matched.is_empty() {
            return Ok(bash);
        }
        bash.push_str("sudo -H pip3 install --upgrade \\\n");
        for (i, m) in matched.iter().enumerate() {
            if i == matched.len() - 1 {
                bash.push_str(&format!("    {}\n", m));
            } else {
                bash.push_str(&format!("    {} \\\n", m));
            }
        }
        bash.push_str("\n");
    }
    Ok(bash)
}

/// pipx
fn render_pipx(spec: &PackageSpec, patterns: &[String]) -> Result<String> {
    let mut bash = String::new();
    bash.push_str("echo \"pipx packages:\" \n");
    if let Some(ref items) = spec.items {
        let matched = filter_by_patterns(items, patterns);
        if matched.is_empty() {
            return Ok(bash);
        }
        bash.push_str("while read pkg; do\n    pipx install \"$pkg\"\ndone<<EOM\n");
        for p in matched {
            bash.push_str(&format!("{}\n", p));
        }
        bash.push_str("EOM\n");
    }
    Ok(bash)
}

/// flatpak
fn render_flatpak(spec: &PackageSpec, patterns: &[String]) -> Result<String> {
    let mut bash = String::new();
    bash.push_str("echo \"flatpak packages:\" \n");
    if let Some(ref items) = spec.items {
        let matched = filter_by_patterns(items, patterns);
        if matched.is_empty() {
            return Ok(bash);
        }
        bash.push_str("flatpak install --assumeyes --or-update \\\n");
        for (i, m) in matched.iter().enumerate() {
            if i == matched.len() - 1 {
                bash.push_str(&format!("    {}\n", m));
            } else {
                bash.push_str(&format!("    {} \\\n", m));
            }
        }
    }
    Ok(bash)
}

/// cargo
fn render_cargo(spec: &PackageSpec, patterns: &[String]) -> Result<String> {
    let mut bash = String::new();
    bash.push_str("echo \"cargo crates:\" \n");
    if let Some(ref items) = spec.items {
        let matched = filter_by_patterns(items, patterns);
        if matched.is_empty() {
            return Ok(bash);
        }
        bash.push_str("cargo install \\\n");
        for (i, m) in matched.iter().enumerate() {
            if i == matched.len() - 1 {
                bash.push_str(&format!("    {}\n", m));
            } else {
                bash.push_str(&format!("    {} \\\n", m));
            }
        }
        bash.push_str("\n");
    }
    Ok(bash)
}

/// github
fn render_github(
    repos: &std::collections::HashMap<String, GithubRepoSpec>,
    patterns: &[String],
    args: &Cli
) -> Result<String> {
    let mut bash = String::new();
    bash.push_str("echo \"github repos:\" \n");

    // gather all keys
    let all_keys: Vec<String> = repos.keys().cloned().collect();
    let matched_keys = filter_by_patterns(&all_keys, patterns);

    for key in matched_keys {
        if let Some(repo_spec) = repos.get(&key) {
            bash.push_str(&format!("echo \"{}:\" \n", key));

            let path = format!("{}/repos/{}", args.cwd, key.replace("/", "_"));
            bash.push_str(&format!(
                "git clone --recursive https://github.com/{} {}\n",
                key, path
            ));
            bash.push_str(&format!("(cd {} && pwd && git pull && git checkout HEAD)\n", path));

            // link
            if let Some(ref link_map) = repo_spec.link {
                bash.push_str("echo \"links:\"\n");
                bash.push_str("while read -r file link; do\n    linker \"$file\" \"$link\"\ndone<<EOM\n");
                for (src, dst) in link_map {
                    let src_abs = format!("{}/{}", path, src);
                    let dst_exp = dst.replace("~", &args.home).replace("$HOME", &args.home);
                    bash.push_str(&format!("{} {}\n", src_abs, dst_exp));
                }
                bash.push_str("EOM\n");
            }

            // script
            if let Some(ref script_body) = repo_spec.script {
                bash.push_str("echo \"script:\" \n");
                bash.push_str(script_body);
                bash.push_str("\n");
            }
        }
    }

    Ok(bash)
}

/// script
fn render_script(
    scripts: &std::collections::HashMap<String, String>,
    patterns: &[String]
) -> Result<String> {
    let mut bash = String::new();
    bash.push_str("echo \"scripts:\" \n");

    let all_keys: Vec<String> = scripts.keys().cloned().collect();
    let matched = filter_by_patterns(&all_keys, patterns);

    for k in matched {
        if let Some(script_body) = scripts.get(&k) {
            bash.push_str(&format!("echo \"{}:\" \n", k));
            bash.push_str(script_body);
            bash.push_str("\n\n");
        }
    }
    Ok(bash)
}

/// The “linker()” function from the Python
fn include_linker_fn() -> String {
    r#"linker() {
    file=$(realpath "$1")
    link="${2/#\~/$HOME}"
    echo "$link -> $file"
    if [ -f "$link" ] && [ "$file" != "$(readlink "$link")" ]; then
        orig="$link.orig"
        mv "$link" "$orig"
    elif [ ! -f "$link" ] && [ -L "$link" ]; then
        unlink "$link"
    fi
    if [ -f "$link" ]; then
        echo "[exists] $link"
    else
        echo "[create] $link -> $file"
        mkdir -p "$(dirname "$link")"; ln -s "$file" "$link"
    fi
}
"#.to_string()
}

/// The “latest()” function from the Python
fn include_latest_fn() -> String {
    r#"latest() {
    PATTERN="$1"
    LATEST="$2"
    NAME="${3:-"$PATTERN"}"
    echo "Fetching latest release from: $LATEST"
    echo "Using pattern: $PATTERN"
    URL="$(curl -sL "$LATEST" | jq -r ".assets[] | select(.name | test(\"$PATTERN\")) | .browser_download_url")"
    if [[ -z "$URL" ]]; then
        echo "No URL found for pattern: $PATTERN"
        exit 1
    fi
    echo "Downloading from URL: $URL"
    FILENAME=$(basename "$URL")
    TMPDIR=$(mktemp -d /tmp/manifest.XXXXXX)
    pushd "$TMPDIR"
    curl -sSL "$URL" -o "$FILENAME"
    echo "Downloaded $FILENAME"
    if [[ "$FILENAME" =~ \.tar\.gz$ ]]; then
        tar xzf "$FILENAME"
    elif [[ "$FILENAME" =~ \.tbz$ ]]; then
        tar xjf "$FILENAME"
    fi
    BINARY=$(find . -type f -name "$NAME" -exec chmod a+x {} + -print)
    if [[ -z "$BINARY" ]]; then
        echo "No binary found named $NAME"
        exit 1
    fi
    mv "$BINARY" ~/bin/
    popd
    rm -rf "$TMPDIR"
}
"#.to_string()
}

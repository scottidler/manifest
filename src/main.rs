// src/main.rs

mod config;
mod manifest;
mod cli; // your existing CLI code
mod fuzzy; // if you have fuzzy code

use crate::cli::Cli;
use crate::config::*;
use crate::manifest::{ManifestType, build_script};
use clap::Parser;
use eyre::Result;
use std::fs::File;
use std::io::BufReader;
use std::collections::HashMap;

fn main() -> Result<()> {
    let cli = Cli::parse();
    env_logger::init();

    // parse the YAML
    let file = File::open(&cli.config)?;
    let mut reader = BufReader::new(file);
    let manifest_spec: ManifestSpec = load_manifest_spec(&mut reader)?;

    let complete = !cli.any_section_specified();

    // We'll gather “sections” in a plain Vec<ManifestType>
    let mut sections: Vec<ManifestType> = Vec::new();

    // 1) Link => Heredoc style => store as Vec<String>:
    if complete || !cli.link.is_empty() {
        if !manifest_spec.link.items.is_empty() || manifest_spec.link.recursive {
            // Convert the LinkSpec items -> Vec<String> for each (src,dst)
            let lines = linkspec_to_vec(&manifest_spec.link, &cli)?;
            println!("lines={:?}", lines);
            sections.push(ManifestType::Link(lines));
        }
    }

    // 2) Ppa => also Heredoc => store as Vec<String>
    if complete || !cli.ppa.is_empty() {
        let items = &manifest_spec.ppa.items;
        if !items.is_empty() {
            sections.push(ManifestType::Ppa(items.clone()));
        }
    }

    // Combine `pkg.items` + `apt.items` or `dnf.items`
    // if pkgmgr=deb => apt
    // if pkgmgr=rpm => dnf
    let pkgmgr = cli.pkgmgr.to_lowercase();
    if pkgmgr == "deb" {
        // APT
        if complete || !cli.apt.is_empty() {
            // Merge
            let merged = merge_pkg_apt(&manifest_spec);
            if !merged.is_empty() {
                sections.push(ManifestType::Apt(merged));
            }
        }
    } else if pkgmgr == "rpm" {
        // DNF
        if complete || !cli.dnf.is_empty() {
            let merged = merge_pkg_dnf(&manifest_spec);
            if !merged.is_empty() {
                sections.push(ManifestType::Dnf(merged));
            }
        }
    }

    // 3) NPM => Continue => store as Vec<String>
    if complete || !cli.npm.is_empty() {
        if !manifest_spec.npm.items.is_empty() {
            sections.push(ManifestType::Npm(manifest_spec.npm.items.clone()));
        }
    }

    // 4) Pip3 => Continue => unify items+distutils
    if complete || !cli.pip3.is_empty() {
        let mut combined = manifest_spec.pip3.items.clone();
        combined.extend_from_slice(&manifest_spec.pip3.distutils);
        if !combined.is_empty() {
            sections.push(ManifestType::Pip3(combined));
        }
    }

    // 5) Pipx => Heredoc
    if complete || !cli.pipx.is_empty() {
        if !manifest_spec.pipx.items.is_empty() {
            sections.push(ManifestType::Pipx(manifest_spec.pipx.items.clone()));
        }
    }

    // 6) Flatpak => Continue
    if complete || !cli.flatpak.is_empty() {
        if !manifest_spec.flatpak.items.is_empty() {
            sections.push(ManifestType::Flatpak(manifest_spec.flatpak.items.clone()));
        }
    }

    // 7) Cargo => Continue
    if complete || !cli.cargo.is_empty() {
        if !manifest_spec.cargo.items.is_empty() {
            sections.push(ManifestType::Cargo(manifest_spec.cargo.items.clone()));
        }
    }

    // 8) Github => custom => store as a HashMap
    if complete || !cli.github.is_empty() {
        if !manifest_spec.github.items.is_empty() {
            // we’re storing "reponame -> ???"
            // But the python code does more complex logic.
            // For demonstration, let's flatten it. Key=repoName, Value=some placeholder
            // Or you can store actual link/dst lines. Up to you.
            let mut map = HashMap::new();
            for (repo_name, _repo_spec) in &manifest_spec.github.items {
                // If you want more detail, you can do so.
                // For now, store "some detail"
                map.insert(repo_name.clone(), "some detail".to_string());
            }
            sections.push(ManifestType::Github(map));
        }
    }

    // 9) Script => custom => store the script items as HashMap
    if complete || !cli.script.is_empty() {
        if !manifest_spec.script.items.is_empty() {
            sections.push(ManifestType::Script(manifest_spec.script.items.clone()));
        }
    }

    // Render final output
    let output = build_script(&sections);
    println!("{}", output);

    Ok(())
}

/// Convert the LinkSpec into a Vec<String> lines, e.g. "src dst"
fn linkspec_to_vec(spec: &LinkSpec, _cli: &Cli) -> Result<Vec<String>> {
    let mut lines = Vec::new();
    for (src, dst) in &spec.items {
        // For example: "src + " " + dst"
        let line = format!("{} {}", src, dst);
        lines.push(line);
    }
    Ok(lines)
}

/// Merges top-level `pkg.items` with `apt.items`
fn merge_pkg_apt(spec: &ManifestSpec) -> Vec<String> {
    let mut merged = Vec::new();
    merged.extend_from_slice(&spec.pkg.items);
    merged.extend_from_slice(&spec.apt.items);
    merged
}

/// Merges top-level `pkg.items` with `dnf.items`
fn merge_pkg_dnf(spec: &ManifestSpec) -> Vec<String> {
    let mut merged = Vec::new();
    merged.extend_from_slice(&spec.pkg.items);
    merged.extend_from_slice(&spec.dnf.items);
    merged
}

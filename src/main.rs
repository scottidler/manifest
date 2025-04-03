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
use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

// Returns a sorted clone of a vector of Strings.
fn sorted_vec(vec: &[String]) -> Vec<String> {
    let mut v = vec.to_vec();
    v.sort();
    v
}

// Returns a new HashMap sorted by key.
// Note: The returned HashMap does not guarantee iteration order,
// so for rendering sorted output you may want to convert the keys to a Vec.
fn sorted_map(map: &HashMap<String, String>) -> HashMap<String, String> {
    let mut keys: Vec<_> = map.keys().collect();
    keys.sort();
    let mut sorted = HashMap::new();
    for key in keys {
        if let Some(val) = map.get(key) {
            sorted.insert(key.clone(), val.clone());
        }
    }
    sorted
}

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

    // 1) Link => Heredoc style => store as Vec<String>
    if complete || !cli.link.is_empty() {
        if !manifest_spec.link.items.is_empty() || manifest_spec.link.recursive {
            let lines = linkspec_to_vec(&manifest_spec.link, &cli)?;
            sections.push(ManifestType::Link(sorted_vec(&lines)));
        }
    }

    // 2) Ppa => Heredoc style => store as Vec<String>
    if complete || !cli.ppa.is_empty() {
        let items = &manifest_spec.ppa.items;
        if !items.is_empty() {
            sections.push(ManifestType::Ppa(sorted_vec(&items.clone())));
        }
    }

    // Combine `pkg.items` + `apt.items` or `dnf.items`
    let pkgmgr = cli.pkgmgr.to_lowercase();
    if pkgmgr == "deb" {
        if complete || !cli.apt.is_empty() {
            let merged = merge_pkg_apt(&manifest_spec);
            if !merged.is_empty() {
                sections.push(ManifestType::Apt(sorted_vec(&merged)));
            }
        }
    } else if pkgmgr == "rpm" {
        if complete || !cli.dnf.is_empty() {
            let merged = merge_pkg_dnf(&manifest_spec);
            if !merged.is_empty() {
                sections.push(ManifestType::Dnf(sorted_vec(&merged)));
            }
        }
    }

    // 3) NPM => Continue style => store as Vec<String>
    if complete || !cli.npm.is_empty() {
        if !manifest_spec.npm.items.is_empty() {
            sections.push(ManifestType::Npm(sorted_vec(&manifest_spec.npm.items.clone())));
        }
    }

    // 4) Pip3 => Continue style => unify items+distutils
    if complete || !cli.pip3.is_empty() {
        let mut combined = manifest_spec.pip3.items.clone();
        combined.extend_from_slice(&manifest_spec.pip3.distutils);
        if !combined.is_empty() {
            sections.push(ManifestType::Pip3(sorted_vec(&combined)));
        }
    }

    // 5) Pipx => Heredoc style => store as Vec<String>
    if complete || !cli.pipx.is_empty() {
        if !manifest_spec.pipx.items.is_empty() {
            sections.push(ManifestType::Pipx(sorted_vec(&manifest_spec.pipx.items.clone())));
        }
    }

    // 6) Flatpak => Continue style => store as Vec<String>
    if complete || !cli.flatpak.is_empty() {
        if !manifest_spec.flatpak.items.is_empty() {
            sections.push(ManifestType::Flatpak(sorted_vec(&manifest_spec.flatpak.items.clone())));
        }
    }

    // 7) Cargo => Continue style => store as Vec<String>
    if complete || !cli.cargo.is_empty() {
        if !manifest_spec.cargo.items.is_empty() {
            sections.push(ManifestType::Cargo(sorted_vec(&manifest_spec.cargo.items.clone())));
        }
    }

    // 8) Github => custom => store as a HashMap<String, String>
    if complete || !cli.github.is_empty() {
        if !manifest_spec.github.items.is_empty() {
            let mut map = HashMap::new();
            for (repo_name, _repo_spec) in &manifest_spec.github.items {
                map.insert(repo_name.clone(), "some detail".to_string());
            }
            sections.push(ManifestType::Github(sorted_map(&map)));
        }
    }

    // 9) Script => custom => store the script items as a HashMap<String, String>
    if complete || !cli.script.is_empty() {
        if !manifest_spec.script.items.is_empty() {
            sections.push(ManifestType::Script(sorted_map(&manifest_spec.script.items.clone())));
        }
    }

    // Render final output
    let output = build_script(&sections);
    println!("{}", output);

    Ok(())
}

/// Converts the LinkSpec into a Vec<String> where each string is a line of "src dst"
/// When recursive is enabled, it traverses the source directory using WalkDir and computes
/// the destination path by appending the file's relative path to the destination prefix.
/// It also performs interpolation of "$HOME" in the destination path.
fn linkspec_to_vec(spec: &LinkSpec, cli: &Cli) -> Result<Vec<String>> {
    let mut lines = Vec::new();
    let cwd = Path::new(&cli.cwd);

    let home = if cli.home.is_empty() {
        std::env::var("HOME")?
    } else {
        cli.home.clone()
    };

    if spec.recursive {
        for (src, dst) in &spec.items {
            let src_dir = cwd.join(src);
            if src_dir.exists() {
                // By default, WalkDir returns the exact on-disk path, including symlinks.
                // We do NOT call `canonicalize`, so we won't resolve symlinks to real paths.
                for entry in WalkDir::new(&src_dir).into_iter().filter_map(|e| e.ok()) {
                    let path = entry.path();
                    if path.is_file() {
                        // Compute the relative path from src_dir to this file.
                        let rel = path
                            .strip_prefix(&src_dir)
                            .unwrap_or(path);

                        // Build the destination path by joining dst with that relative path.
                        let dst_path: PathBuf = Path::new(dst).join(rel);

                        // Replace $HOME in the destination path if needed.
                        let mut final_dst = dst_path.to_string_lossy().to_string();
                        final_dst = final_dst.replace("$HOME", &home);

                        // Convert the source path to a string as-is (preserving symlinks).
                        let source_str = path.to_string_lossy().to_string();

                        lines.push(format!("{} {}", source_str, final_dst));
                    }
                }
            }
        }
    } else {
        for (src, dst) in &spec.items {
            // If you want to keep symlinks in `src` as typed in your YAML, do NOT canonicalize:
            let source_path = cwd.join(src);
            let source_str = source_path.to_string_lossy().to_string();

            let dst_path = Path::new(dst).to_path_buf();
            let mut final_dst = dst_path.to_string_lossy().to_string();
            final_dst = final_dst.replace("$HOME", &home);

            lines.push(format!("{} {}", source_str, final_dst));
        }
    }
    Ok(lines)
}

fn merge_pkg_apt(spec: &ManifestSpec) -> Vec<String> {
    let mut merged = Vec::new();
    merged.extend_from_slice(&spec.pkg.items);
    merged.extend_from_slice(&spec.apt.items);
    merged
}

fn merge_pkg_dnf(spec: &ManifestSpec) -> Vec<String> {
    let mut merged = Vec::new();
    merged.extend_from_slice(&spec.pkg.items);
    merged.extend_from_slice(&spec.dnf.items);
    merged
}

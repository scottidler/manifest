// src/main.rs

mod config;
mod manifest;
mod cli;
mod fuzzy;

use crate::cli::Cli;
use crate::config::*;
use crate::manifest::{ManifestType, build_script};
use crate::fuzzy::fuzzy;
use clap::Parser;
use eyre::Result;
use log::*;
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{BufReader, Write};
use std::path::Path;
use std::process::Command;
use walkdir::WalkDir;
use chrono::Local;

/// Checks whether the given program is available on the system by invoking "command -v".
fn check_hash(program: &str) -> bool {
    debug!("check_hash: checking for program {}", program);
    let output = Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {}", program))
        .output();
    match output {
        Ok(o) => {
            let found = !o.stdout.is_empty();
            debug!("check_hash: program {} found = {}", program, found);
            found
        }
        Err(e) => {
            warn!("check_hash: error checking {}: {}", program, e);
            false
        }
    }
}

/// Determines the package manager in use by querying the system.
fn get_pkgmgr() -> Result<String> {
    if check_hash("dpkg") {
        debug!("get_pkgmgr: detected dpkg");
        Ok("deb".to_string())
    } else if check_hash("rpm") {
        debug!("get_pkgmgr: detected rpm");
        Ok("rpm".to_string())
    } else if check_hash("brew") {
        debug!("get_pkgmgr: detected brew");
        Ok("brew".to_string())
    } else {
        Err(eyre::eyre!("unknown pkgmgr!"))
    }
}

/// Returns a sorted clone of a vector of Strings.
fn sorted_vec(vec: &[String]) -> Vec<String> {
    debug!("sorted_vec: received input vector with {} items", vec.len());
    let mut v = vec.to_vec();
    v.sort();
    debug!("sorted_vec: sorted vector = {:?}", v);
    v
}

/// Returns a new HashMap sorted by key.
fn sorted_map(map: &HashMap<String, String>) -> HashMap<String, String> {
    debug!("sorted_map: received map with {} entries", map.len());
    let mut keys: Vec<_> = map.keys().collect();
    keys.sort();
    let mut sorted = HashMap::new();
    for key in keys {
        if let Some(val) = map.get(key) {
            sorted.insert(key.clone(), val.clone());
        }
    }
    debug!("sorted_map: sorted map keys = {:?}", sorted.keys().collect::<Vec<_>>());
    sorted
}

/// Converts the LinkSpec into a Vec<String> where each string is a line of "src dst"
fn linkspec_to_vec(spec: &config::LinkSpec, cli: &Cli) -> Result<Vec<String>> {
    debug!("linkspec_to_vec: starting with spec = {:?}", spec);
    let mut lines = Vec::new();
    let cwd = Path::new(&cli.cwd);
    debug!("linkspec_to_vec: current working directory = {:?}", cwd);

    let home = if cli.home.is_empty() {
        std::env::var("HOME").unwrap_or_else(|_| ".".to_string())
    } else {
        cli.home.clone()
    };
    debug!("linkspec_to_vec: resolved HOME = {}", home);

    if spec.recursive {
        debug!("linkspec_to_vec: recursive mode enabled");
        for (src, dst) in &spec.items {
            let src_dir = cwd.join(src);
            debug!("linkspec_to_vec: processing src = {:?} -> dst = {:?}", src_dir, dst);
            if src_dir.exists() {
                for entry in WalkDir::new(&src_dir).into_iter().filter_map(|e| e.ok()) {
                    let path = entry.path();
                    if path.is_file() {
                        let rel = path.strip_prefix(&src_dir).unwrap_or(path);
                        let dst_path = Path::new(dst).join(rel);
                        let mut final_dst = dst_path.to_string_lossy().to_string();
                        final_dst = final_dst.replace("$HOME", &home);
                        let source_str = path.to_string_lossy().to_string();
                        lines.push(format!("{} {}", source_str, final_dst));
                    }
                }
            } else {
                warn!("linkspec_to_vec: source directory {:?} does not exist", src_dir);
            }
        }
    } else {
        debug!("linkspec_to_vec: non-recursive mode");
        for (src, dst) in &spec.items {
            let source_path = cwd.join(src);
            let source_str = source_path.to_string_lossy().to_string();
            let dst_path = Path::new(dst).to_path_buf();
            let mut final_dst = dst_path.to_string_lossy().to_string();
            final_dst = final_dst.replace("$HOME", &home);
            lines.push(format!("{} {}", source_str, final_dst));
        }
    }
    debug!("linkspec_to_vec: generated {} lines", lines.len());
    Ok(lines)
}

fn merge_pkg_apt(spec: &ManifestSpec) -> Vec<String> {
    debug!(
        "merge_pkg_apt: merging pkg.items (len={}) and apt.items (len={})",
        spec.pkg.items.len(),
        spec.apt.items.len()
    );
    let mut merged = Vec::new();
    merged.extend_from_slice(&spec.pkg.items);
    merged.extend_from_slice(&spec.apt.items);
    debug!("merge_pkg_apt: merged length = {}", merged.len());
    merged
}

fn merge_pkg_dnf(spec: &ManifestSpec) -> Vec<String> {
    debug!(
        "merge_pkg_dnf: merging pkg.items (len={}) and dnf.items (len={})",
        spec.pkg.items.len(),
        spec.dnf.items.len()
    );
    let mut merged = Vec::new();
    merged.extend_from_slice(&spec.pkg.items);
    merged.extend_from_slice(&spec.dnf.items);
    debug!("merge_pkg_dnf: merged length = {}", merged.len());
    merged
}

fn main() -> Result<()> {
    use env_logger::Builder;
    use log::LevelFilter;

    let mut log_builder = Builder::new();
    log_builder.filter_level(LevelFilter::Info);
    if let Ok(rust_log) = std::env::var("RUST_LOG") {
        log_builder.parse_filters(&rust_log);
    }
    let log_file_path = std::env::var("HOME")
        .map(|home| format!("{}/manifest.log", home))
        .unwrap_or_else(|_| "manifest.log".to_string());
    let file = OpenOptions::new()
        .append(true)
        .create(true)
        .open(&log_file_path)
        .expect("Unable to open log file");
    writeln!(
        &file,
        "\n================ New run at {} ================",
        Local::now()
    )
    .expect("Unable to write log separator");
    log_builder.target(env_logger::Target::Pipe(Box::new(file)));
    log_builder.init();

    info!("Starting manifest generation");

    let cli = Cli::parse();
    debug!("Parsed CLI arguments: {:?}", cli);

    // Parse the YAML config.
    let file = File::open(&cli.config)?;
    debug!("Opened config file: {}", cli.config);
    let mut reader = BufReader::new(file);
    let manifest_spec: ManifestSpec = config::load_manifest_spec(&mut reader)?;
    debug!("Loaded manifest spec: {:?}", manifest_spec);

    let complete = !cli.any_section_specified();
    debug!("Complete mode = {}", complete);

    let pkgmgr = get_pkgmgr()?;
    debug!("Determined pkgmgr: {}", pkgmgr);

    let mut sections: Vec<ManifestType> = Vec::new();

    // 1) Link section.
    if complete || !cli.link.is_empty() {
        if !manifest_spec.link.items.is_empty() || manifest_spec.link.recursive {
            let lines = linkspec_to_vec(&manifest_spec.link, &cli)?;
            debug!("Adding Link section with {} lines", lines.len());
            sections.push(ManifestType::Link(sorted_vec(&lines)));
        }
    }

    // 2) PPA section.
    if complete || !cli.ppa.is_empty() {
        let items = &manifest_spec.ppa.items;
        if !items.is_empty() {
            debug!("Adding Ppa section with {} items", items.len());
            sections.push(ManifestType::Ppa(sorted_vec(&items.clone())));
        }
    }

    // 3) Apt/Dnf section.
    if pkgmgr == "deb" {
        if complete || !cli.apt.is_empty() {
            let merged = merge_pkg_apt(&manifest_spec);
            if !merged.is_empty() {
                debug!("Adding Apt section with {} merged items", merged.len());
                sections.push(ManifestType::Apt(sorted_vec(&merged)));
            }
        }
    } else if pkgmgr == "rpm" {
        if complete || !cli.dnf.is_empty() {
            let merged = merge_pkg_dnf(&manifest_spec);
            if !merged.is_empty() {
                debug!("Adding Dnf section with {} merged items", merged.len());
                sections.push(ManifestType::Dnf(sorted_vec(&merged)));
            }
        }
    }

    // 4) NPM section.
    if complete || !cli.npm.is_empty() {
        if !manifest_spec.npm.items.is_empty() {
            debug!("Adding Npm section with {} items", manifest_spec.npm.items.len());
            sections.push(ManifestType::Npm(sorted_vec(&manifest_spec.npm.items.clone())));
        }
    }

    // 5) Pip3 section.
    if complete || !cli.pip3.is_empty() {
        let mut combined = manifest_spec.pip3.items.clone();
        combined.extend_from_slice(&manifest_spec.pip3.distutils);
        if !combined.is_empty() {
            debug!("Adding Pip3 section with {} combined items", combined.len());
            sections.push(ManifestType::Pip3(sorted_vec(&combined)));
        }
    }

    // 6) Pipx section.
    if complete || !cli.pipx.is_empty() {
        if !manifest_spec.pipx.items.is_empty() {
            debug!("Adding Pipx section with {} items", manifest_spec.pipx.items.len());
            sections.push(ManifestType::Pipx(sorted_vec(&manifest_spec.pipx.items.clone())));
        }
    }

    // 7) Flatpak section.
    if complete || !cli.flatpak.is_empty() {
        if !manifest_spec.flatpak.items.is_empty() {
            debug!("Adding Flatpak section with {} items", manifest_spec.flatpak.items.len());
            sections.push(ManifestType::Flatpak(sorted_vec(&manifest_spec.flatpak.items.clone())));
        }
    }

    // 8) Cargo section.
    if complete || !cli.cargo.is_empty() {
        if !manifest_spec.cargo.items.is_empty() {
            debug!("Adding Cargo section with {} items", manifest_spec.cargo.items.len());
            sections.push(ManifestType::Cargo(sorted_vec(&manifest_spec.cargo.items.clone())));
        }
    }

    // 9) Github section.
    if complete || !cli.github.is_empty() {
        let matched: HashMap<String, RepoSpec> =
            fuzzy(manifest_spec.github.items.clone());
        if !matched.is_empty() {
            debug!("Adding Github section with {} repos", matched.len());
            sections.push(ManifestType::Github(matched));
        }
    }

    // 10) Script section.
    if complete || !cli.script.is_empty() {
        if !manifest_spec.script.items.is_empty() {
            debug!("Adding Script section with {} items", manifest_spec.script.items.len());
            sections.push(ManifestType::Script(sorted_map(&manifest_spec.script.items.clone())));
        }
    }

    debug!("Total sections collected: {}", sections.len());
    let output = build_script(&sections);
    debug!("Generated output script:\n{}", output);
    println!("{}", output);

    info!("Manifest generation completed");
    Ok(())
}

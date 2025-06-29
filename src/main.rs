// src/main.rs

mod config;
mod manifest;
mod cli;
mod fuzzy;

use crate::cli::Cli;
use crate::config::*;
use crate::manifest::{ManifestType, build_script};
use crate::fuzzy::*;
use clap::Parser;
use eyre::Result;
use eyre::WrapErr;
use log::*;
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{BufReader, Write};
use std::path::Path;
use std::process::Command;
use walkdir::WalkDir;
use chrono::Local;
use colored::*;
use clap::CommandFactory;

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

fn sorted_vec(vec: &[String]) -> Vec<String> {
    debug!("sorted_vec: received input vector with {} items", vec.len());
    let mut v = vec.to_vec();
    v.sort();
    debug!("sorted_vec: sorted vector = {:?}", v);
    v
}

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

fn linkspec_to_vec(spec: &config::LinkSpec, cli: &Cli) -> Result<Vec<String>> {
    debug!("linkspec_to_vec: starting with spec = {:?}", spec);
    let mut lines = Vec::new();
    let cwd = Path::new(&cli.path);
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

fn ensure_manifest_functions() -> Result<()> {
    ensure_manifest_functions_with_home_and_bin(None, "bin")
}

fn ensure_manifest_functions_with_home_and_bin(home_override: Option<&str>, bin_dir: &str) -> Result<()> {
    let home_dir = match home_override {
        Some(home) => home.to_string(),
        None => std::env::var("HOME").map_err(|e| eyre::eyre!("HOME environment variable not set: {}", e))?,
    };

    let manifest_dir = format!("{}/.local/share/manifest", home_dir);

    // Check if bin directory exists
    if !std::path::Path::new(bin_dir).exists() {
        return Ok(());
    }

    // Scan for .sh files in bin/
    let bin_entries = std::fs::read_dir(bin_dir)?;
    let mut shell_files = Vec::new();

    for entry in bin_entries {
        let entry = entry?;
        let path = entry.path();
        if let Some(extension) = path.extension() {
            if extension == "sh" {
                if let Some(filename) = path.file_name() {
                    shell_files.push(filename.to_string_lossy().to_string());
                }
            }
        }
    }

    if shell_files.is_empty() {
        return Ok(());
    }

    // Create manifest directory if it doesn't exist
    std::fs::create_dir_all(&manifest_dir)?;

    let mut installed_files = Vec::new();

    for filename in shell_files {
        let src_path = format!("{}/{}", bin_dir, filename);
        let dest_path = format!("{}/{}", manifest_dir, filename);

        // Only install if the file doesn't already exist
        if !std::path::Path::new(&dest_path).exists() {
            let content = std::fs::read_to_string(&src_path)?;
            std::fs::write(&dest_path, content)?;
            installed_files.push(filename);
        }
    }

    if !installed_files.is_empty() {
        println!("Installed manifest shell functions: {}", installed_files.join(", "));
    }

    Ok(())
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

    // Ensure shell function files are installed
    ensure_manifest_functions().wrap_err("Failed to ensure manifest function files")?;

    let cli = Cli::parse();
    debug!("Parsed CLI arguments: {:?}", cli);

    let file = match File::open(&cli.config) {
        Ok(f) => f,
        Err(e) => {
            if e.kind() == std::io::ErrorKind::NotFound {
                Cli::command().print_help().unwrap();
                println!("");
            }
            eprintln!("Error: Failed to open config file: {}", cli.config.red());
            std::process::exit(1);
        }
    };
    debug!("Opened config file: {}", cli.config);
    let mut reader = BufReader::new(file);
    let manifest_spec: ManifestSpec = config::load_manifest_spec(&mut reader).wrap_err("Failed to load manifest spec")?;
    debug!("Loaded manifest spec: {:?}", manifest_spec);

    let complete = !cli.any_section_specified();
    debug!("Complete mode = {}", complete);

    let pkgmgr = get_pkgmgr().wrap_err("Failed to determine package manager")?;
    debug!("Determined pkgmgr: {}", pkgmgr);

    let mut sections: Vec<ManifestType> = Vec::new();

    if complete || !cli.link.is_empty() {
        if !manifest_spec.link.items.is_empty() || manifest_spec.link.recursive {
            let lines = linkspec_to_vec(&manifest_spec.link, &cli)?;
            let filtered = fuzzy(lines).include(&cli.link);
            debug!("Adding Link section with {} lines", filtered.len());
            sections.push(ManifestType::Link(sorted_vec(&filtered)));
        }
    }

    if complete || !cli.ppa.is_empty() {
        let ppa_items = fuzzy(manifest_spec.ppa.items.clone()).include(&cli.ppa);
        if !ppa_items.is_empty() {
            debug!("Adding Ppa section with {} items", ppa_items.len());
            sections.push(ManifestType::Ppa(sorted_vec(&ppa_items)));
        }
    }

    if pkgmgr == "deb" {
        if complete || !cli.apt.is_empty() {
            let merged = merge_pkg_apt(&manifest_spec);
            let apt_items = fuzzy(merged).include(&cli.apt);
            if !apt_items.is_empty() {
                debug!("Adding Apt section with {} merged items", apt_items.len());
                sections.push(ManifestType::Apt(sorted_vec(&apt_items)));
            }
        }
    } else if pkgmgr == "rpm" {
        if complete || !cli.dnf.is_empty() {
            let merged = merge_pkg_dnf(&manifest_spec);
            let dnf_items = fuzzy(merged).include(&cli.dnf);
            if !dnf_items.is_empty() {
                debug!("Adding Dnf section with {} merged items", dnf_items.len());
                sections.push(ManifestType::Dnf(sorted_vec(&dnf_items)));
            }
        }
    }

    if complete || !cli.npm.is_empty() {
        let npm_items = fuzzy(manifest_spec.npm.items.clone()).include(&cli.npm);
        if !npm_items.is_empty() {
            debug!("Adding Npm section with {} items", npm_items.len());
            sections.push(ManifestType::Npm(sorted_vec(&npm_items)));
        }
    }

    if complete || !cli.pip3.is_empty() {
        let mut combined = manifest_spec.pip3.items.clone();
        combined.extend_from_slice(&manifest_spec.pip3.distutils);
        let pip3_items = fuzzy(combined).include(&cli.pip3);
        if !pip3_items.is_empty() {
            debug!("Adding Pip3 section with {} combined items", pip3_items.len());
            sections.push(ManifestType::Pip3(sorted_vec(&pip3_items)));
        }
    }

    if complete || !cli.pipx.is_empty() {
        let pipx_items = fuzzy(manifest_spec.pipx.items.clone()).include(&cli.pipx);
        if !pipx_items.is_empty() {
            debug!("Adding Pipx section with {} items", pipx_items.len());
            sections.push(ManifestType::Pipx(sorted_vec(&pipx_items)));
        }
    }

    if complete || !cli.flatpak.is_empty() {
        let flatpak_items = fuzzy(manifest_spec.flatpak.items.clone()).include(&cli.flatpak);
        if !flatpak_items.is_empty() {
            debug!("Adding Flatpak section with {} items", flatpak_items.len());
            sections.push(ManifestType::Flatpak(sorted_vec(&flatpak_items)));
        }
    }

    if complete || !cli.cargo.is_empty() {
        let cargo_items = fuzzy(manifest_spec.cargo.items.clone()).include(&cli.cargo);
        if !cargo_items.is_empty() {
            debug!("Adding Cargo section with {} items", cargo_items.len());
            sections.push(ManifestType::Cargo(sorted_vec(&cargo_items)));
        }
    }

    if complete || !cli.github.is_empty() {
        let github_items: HashMap<String, RepoSpec> =
            fuzzy(manifest_spec.github.items.clone()).include(&cli.github);
        if !github_items.is_empty() {
            debug!("Adding Github section with {} repos", github_items.len());
            sections.push(ManifestType::Github(github_items, manifest_spec.github.repopath.clone()));
        }
    }

    if complete || !cli.git_crypt.is_empty() {
        let gitcrypt_items: HashMap<String, RepoSpec> =
            fuzzy(manifest_spec.git_crypt.items.clone()).include(&cli.git_crypt);
        if !gitcrypt_items.is_empty() {
            debug!("Adding GitCrypt section with {} repos", gitcrypt_items.len());
            sections.push(ManifestType::GitCrypt(gitcrypt_items, manifest_spec.git_crypt.repopath.clone()));
        }
    }

    if complete || !cli.script.is_empty() {
        let script_items = fuzzy(manifest_spec.script.items.clone()).include(&cli.script);
        if !script_items.is_empty() {
            debug!("Adding Script section with {} items", script_items.len());
            sections.push(ManifestType::Script(sorted_map(&script_items)));
        }
    }

    debug!("Total sections collected: {}", sections.len());
    let output = build_script(&sections);
    debug!("Generated output script:\n{}", output);
    println!("{}", output);

    info!("Manifest generation completed");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn setup_test_env() -> (TempDir, String) {
        let temp_home = TempDir::new().unwrap();
        let home_path = temp_home.path().to_string_lossy().to_string();
        (temp_home, home_path)
    }

    fn create_bin_dir_with_files(files: &[(&str, &str)]) -> TempDir {
        let temp_bin = TempDir::new().unwrap();
        let bin_dir = temp_bin.path().join("bin");
        fs::create_dir(&bin_dir).unwrap();

        for (filename, content) in files {
            let file_path = bin_dir.join(filename);
            fs::write(file_path, content).unwrap();
        }

        temp_bin
    }

    fn manifest_dir_path(home: &str) -> String {
        format!("{}/.local/share/manifest", home)
    }

    fn file_exists_in_manifest_dir(home: &str, filename: &str) -> bool {
        let path = format!("{}/{}", manifest_dir_path(home), filename);
        std::path::Path::new(&path).exists()
    }

    fn read_file_from_manifest_dir(home: &str, filename: &str) -> String {
        let path = format!("{}/{}", manifest_dir_path(home), filename);
        fs::read_to_string(path).unwrap()
    }

    #[test]
    fn test_ensure_manifest_functions_creates_directory() {
        let (_temp_home, home_path) = setup_test_env();
        let temp_bin = create_bin_dir_with_files(&[("test.sh", "echo 'test'")]);
        let bin_path = temp_bin.path().join("bin").to_string_lossy().to_string();

        // Run the function with home and bin overrides
        ensure_manifest_functions_with_home_and_bin(Some(&home_path), &bin_path).unwrap();

        // Check that directory was created
        let manifest_dir = manifest_dir_path(&home_path);
        assert!(std::path::Path::new(&manifest_dir).exists());
    }

    #[test]
    fn test_ensure_manifest_functions_installs_single_file() {
        let (_temp_home, home_path) = setup_test_env();
        let test_content = "test_function() {\n  echo 'Hello from test'\n}";
        let temp_bin = create_bin_dir_with_files(&[("test.sh", test_content)]);
        let bin_path = temp_bin.path().join("bin").to_string_lossy().to_string();

        ensure_manifest_functions_with_home_and_bin(Some(&home_path), &bin_path).unwrap();

        // Check file was installed
        assert!(file_exists_in_manifest_dir(&home_path, "test.sh"));

        // Check content is correct
        let installed_content = read_file_from_manifest_dir(&home_path, "test.sh");
        assert_eq!(installed_content, test_content);
    }

    #[test]
    fn test_ensure_manifest_functions_installs_multiple_files() {
        let (_temp_home, home_path) = setup_test_env();

        let linker_content = "linker() {\n  echo 'linker function'\n}";
        let latest_content = "latest() {\n  echo 'latest function'\n}";
        let helper_content = "helper() {\n  echo 'helper function'\n}";

        let temp_bin = create_bin_dir_with_files(&[
            ("linker.sh", linker_content),
            ("latest.sh", latest_content),
            ("helper.sh", helper_content),
        ]);
        let bin_path = temp_bin.path().join("bin").to_string_lossy().to_string();

        ensure_manifest_functions_with_home_and_bin(Some(&home_path), &bin_path).unwrap();

        // Check all files were installed
        assert!(file_exists_in_manifest_dir(&home_path, "linker.sh"));
        assert!(file_exists_in_manifest_dir(&home_path, "latest.sh"));
        assert!(file_exists_in_manifest_dir(&home_path, "helper.sh"));

        // Check content is correct for all files
        assert_eq!(read_file_from_manifest_dir(&home_path, "linker.sh"), linker_content);
        assert_eq!(read_file_from_manifest_dir(&home_path, "latest.sh"), latest_content);
        assert_eq!(read_file_from_manifest_dir(&home_path, "helper.sh"), helper_content);
    }

    #[test]
    fn test_ensure_manifest_functions_skips_existing_files() {
        let (_temp_home, home_path) = setup_test_env();

        let original_content = "original_function() {\n  echo 'original'\n}";
        let new_content = "new_function() {\n  echo 'new'\n}";

        let temp_bin = create_bin_dir_with_files(&[("test.sh", new_content)]);
        let bin_path = temp_bin.path().join("bin").to_string_lossy().to_string();

        // First run - install the file
        ensure_manifest_functions_with_home_and_bin(Some(&home_path), &bin_path).unwrap();
        assert!(file_exists_in_manifest_dir(&home_path, "test.sh"));

        // Manually modify the installed file
        let manifest_file_path = format!("{}/test.sh", manifest_dir_path(&home_path));
        fs::write(&manifest_file_path, original_content).unwrap();

        // Second run - should not overwrite existing file
        ensure_manifest_functions_with_home_and_bin(Some(&home_path), &bin_path).unwrap();

        // Check that the file was not overwritten
        let content = read_file_from_manifest_dir(&home_path, "test.sh");
        assert_eq!(content, original_content);
    }

    #[test]
    fn test_ensure_manifest_functions_installs_only_missing_files() {
        let (_temp_home, home_path) = setup_test_env();

        let existing_content = "existing() {\n  echo 'exists'\n}";
        let new_content = "new() {\n  echo 'new'\n}";

        let temp_bin = create_bin_dir_with_files(&[
            ("existing.sh", existing_content),
            ("new.sh", new_content),
        ]);
        let bin_path = temp_bin.path().join("bin").to_string_lossy().to_string();

        // Pre-install one file manually
        let manifest_dir = manifest_dir_path(&home_path);
        fs::create_dir_all(&manifest_dir).unwrap();
        fs::write(format!("{}/existing.sh", manifest_dir), existing_content).unwrap();

        // Run the function
        ensure_manifest_functions_with_home_and_bin(Some(&home_path), &bin_path).unwrap();

        // Check that both files exist
        assert!(file_exists_in_manifest_dir(&home_path, "existing.sh"));
        assert!(file_exists_in_manifest_dir(&home_path, "new.sh"));

        // Check content is correct
        assert_eq!(read_file_from_manifest_dir(&home_path, "existing.sh"), existing_content);
        assert_eq!(read_file_from_manifest_dir(&home_path, "new.sh"), new_content);
    }

    #[test]
    fn test_ensure_manifest_functions_ignores_non_sh_files() {
        let (_temp_home, home_path) = setup_test_env();

        let temp_bin = create_bin_dir_with_files(&[
            ("script.sh", "echo 'shell script'"),
            ("readme.txt", "This is a readme"),
            ("config.json", "{}"),
            ("another.sh", "echo 'another shell script'"),
        ]);
        let bin_path = temp_bin.path().join("bin").to_string_lossy().to_string();

        ensure_manifest_functions_with_home_and_bin(Some(&home_path), &bin_path).unwrap();

        // Check only .sh files were installed
        assert!(file_exists_in_manifest_dir(&home_path, "script.sh"));
        assert!(file_exists_in_manifest_dir(&home_path, "another.sh"));
        assert!(!file_exists_in_manifest_dir(&home_path, "readme.txt"));
        assert!(!file_exists_in_manifest_dir(&home_path, "config.json"));
    }

    #[test]
    fn test_ensure_manifest_functions_handles_missing_bin_directory() {
        let temp_dir = TempDir::new().unwrap();
        let (_temp_home, home_path) = setup_test_env();

        // Use a non-existent bin directory path
        let non_existent_bin = temp_dir.path().join("non_existent_bin").to_string_lossy().to_string();

        // Should not panic and should return Ok
        let result = ensure_manifest_functions_with_home_and_bin(Some(&home_path), &non_existent_bin);
        assert!(result.is_ok());

        // Manifest directory should not be created
        let manifest_dir = manifest_dir_path(&home_path);
        assert!(!std::path::Path::new(&manifest_dir).exists());
    }

    #[test]
    fn test_ensure_manifest_functions_handles_empty_bin_directory() {
        let (_temp_home, home_path) = setup_test_env();
        let temp_bin = TempDir::new().unwrap();
        let bin_dir = temp_bin.path().join("bin");
        fs::create_dir(&bin_dir).unwrap();
        let bin_path = bin_dir.to_string_lossy().to_string();

        // bin/ directory exists but is empty
        ensure_manifest_functions_with_home_and_bin(Some(&home_path), &bin_path).unwrap();

        // Manifest directory should not be created when no .sh files exist
        let manifest_dir = manifest_dir_path(&home_path);
        assert!(!std::path::Path::new(&manifest_dir).exists());
    }

    #[test]
    fn test_ensure_manifest_functions_with_special_characters_in_filename() {
        let (_temp_home, home_path) = setup_test_env();

        let content = "special() {\n  echo 'special chars'\n}";
        let temp_bin = create_bin_dir_with_files(&[("test-file_v1.2.sh", content)]);
        let bin_path = temp_bin.path().join("bin").to_string_lossy().to_string();

        ensure_manifest_functions_with_home_and_bin(Some(&home_path), &bin_path).unwrap();

        assert!(file_exists_in_manifest_dir(&home_path, "test-file_v1.2.sh"));
        assert_eq!(read_file_from_manifest_dir(&home_path, "test-file_v1.2.sh"), content);
    }
}

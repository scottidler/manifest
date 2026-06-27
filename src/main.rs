// src/main.rs

mod age;
mod cli;
mod config;
mod fuzzy;
mod manifest;

use crate::cli::{AgeAction, Cli, Commands, DecryptFormat};
use crate::config::*;
use crate::fuzzy::*;
use crate::manifest::{ManifestType, build_script};
use chrono::Local;
use clap::Parser;
use eyre::Result;
use eyre::WrapErr;
use log::*;
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

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

fn linkspec_to_vec(spec: &config::LinkSpec, repo_root: &Path, cli: &Cli) -> Result<Vec<String>> {
    debug!("linkspec_to_vec: starting with spec = {:?}", spec);
    let mut lines = Vec::new();
    let cwd = repo_root;
    debug!("linkspec_to_vec: repo root = {:?}", cwd);

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
                        // Skip files whose source path falls under a dirs: entry - those
                        // subtrees are covered by a directory-level symlink instead.
                        let rel_from_cwd = path.strip_prefix(cwd).unwrap_or(path);
                        let rel_str = rel_from_cwd.to_string_lossy();
                        if spec.dirs.keys().any(|d| rel_str.starts_with(d.as_str())) {
                            continue;
                        }
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
    // Process dirs: each entry produces a single directory-level symlink.
    // WalkDir is bypassed entirely - the source path goes straight to linker.
    for (src, dst) in &spec.dirs {
        let source_path = cwd.join(src);
        let source_str = source_path.to_string_lossy().to_string();
        let final_dst = dst.replace("$HOME", &home);
        lines.push(format!("{} {}", source_str, final_dst));
        debug!("linkspec_to_vec: dirs entry {} -> {}", source_str, final_dst);
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
    ensure_manifest_functions_with_home(None)?;
    Ok(())
}

/// Write the embedded shell helpers to the manifest data dir, refreshing any that
/// are absent or whose on-disk content has drifted from the embedded copy. The
/// binary is the single source of truth, so edits to `bin/*.sh` reach the file the
/// generated script sources. Returns the filenames written this call.
///
/// `home_override` is for tests: `Some(home)` resolves the data dir to
/// `<home>/.local/share` deterministically; `None` (production) uses
/// `config::xdg_data_dir()`, which honors `$XDG_DATA_HOME` and falls back to
/// `$HOME/.local/share` on every platform (Linux and macOS alike).
fn ensure_manifest_functions_with_home(home_override: Option<&str>) -> Result<Vec<String>> {
    debug!("ensure_manifest_functions_with_home: home_override={:?}", home_override);
    let data_dir = match home_override {
        Some(home) => PathBuf::from(home).join(".local").join("share"),
        None => config::xdg_data_dir(),
    };
    let manifest_dir = data_dir.join("manifest");
    std::fs::create_dir_all(&manifest_dir)?;

    let mut written = Vec::new();
    for (name, content) in manifest::HELPERS {
        let dest = manifest_dir.join(name);
        let needs_write = match std::fs::read_to_string(&dest) {
            Ok(existing) => existing != *content,
            Err(_) => true,
        };
        if needs_write {
            std::fs::write(&dest, content)?;
            written.push((*name).to_string());
        }
    }

    debug!(
        "ensure_manifest_functions_with_home: wrote {:?} to {:?}",
        written, manifest_dir
    );
    if !written.is_empty() {
        println!("Installed manifest shell functions: {}", written.join(", "));
    }

    Ok(written)
}

fn setup_logging() -> Result<()> {
    use env_logger::Target;

    let log_dir = config::xdg_data_dir().join("manifest").join("logs");

    std::fs::create_dir_all(&log_dir)?;
    let log_file_path = log_dir.join("manifest.log");

    let log_file = OpenOptions::new().create(true).append(true).open(&log_file_path)?;

    writeln!(
        &log_file,
        "\n================ New run at {} ================",
        Local::now()
    )?;

    env_logger::Builder::from_env(env_logger::Env::default().filter_or("RUST_LOG", "info"))
        .target(Target::Pipe(Box::new(log_file)))
        .init();

    Ok(())
}

fn handle_age_command(
    identity: Option<String>,
    recipient: Option<String>,
    keygen: bool,
    public_key: bool,
    action: Option<AgeAction>,
) -> Result<()> {
    // Handle --keygen
    if keygen {
        let output = age::generate_identity()?;
        println!("{}", output);
        return Ok(());
    }

    // Handle --public-key
    if public_key {
        let identity_path = if let Some(path) = &identity {
            std::path::PathBuf::from(path)
        } else {
            let home = std::env::var("HOME").wrap_err("HOME environment variable not set")?;
            let candidates = [
                format!("{}/.config/manifest/identity.txt", home),
                format!("{}/.ssh/id_ed25519", home),
                format!("{}/.ssh/id_rsa", home),
            ];

            candidates
                .iter()
                .find(|p| std::path::Path::new(p).exists())
                .map(std::path::PathBuf::from)
                .ok_or_else(|| eyre::eyre!("No identity file found"))?
        };

        let pubkey = age::get_public_key(&identity_path)?;
        println!("{}", pubkey);
        return Ok(());
    }

    match action {
        Some(AgeAction::Decrypt { path, format }) => {
            let identity_ref = age::resolve_identity(identity.as_deref())?;
            let path = std::path::Path::new(&path);
            let output = match format {
                DecryptFormat::Export => age::render_exports(path, identity_ref.as_ref()),
                DecryptFormat::Env => age::render_env(path, identity_ref.as_ref()),
            };
            print!("{}", output);
            Ok(())
        }
        Some(AgeAction::Encrypt { inputs, output_dir }) => {
            let recipient_box = age::resolve_recipient(recipient.as_deref(), identity.as_deref())?;

            // Classify inputs and reject mixed modes
            let mut has_files = false;
            let mut has_kv = false;
            let mut has_stdin = false;
            for input in &inputs {
                if input == "-" {
                    has_stdin = true;
                } else if Path::new(input).exists() {
                    has_files = true;
                } else if input.contains('=') {
                    has_kv = true;
                }
                // else: will error during processing
            }
            if (has_files && has_kv) || (has_stdin && (has_files || has_kv)) {
                return Err(eyre::eyre!(
                    "Cannot mix file paths, KEY=VAL pairs, and stdin in a single invocation"
                ));
            }

            for input in &inputs {
                if input == "-" {
                    let ciphertext = age::encrypt_stdin(recipient_box.as_ref())?;
                    std::io::Write::write_all(&mut std::io::stdout(), &ciphertext)?;
                } else if Path::new(input).exists() {
                    if inputs.len() == 1 {
                        let ciphertext = age::encrypt_file(Path::new(input), recipient_box.as_ref())?;
                        std::io::Write::write_all(&mut std::io::stdout(), &ciphertext)?;
                    } else {
                        let ciphertext = age::encrypt_file(Path::new(input), recipient_box.as_ref())?;
                        let stem = Path::new(input).file_stem().unwrap_or_default().to_string_lossy();
                        let out_path = Path::new(&output_dir).join(format!("{}.age", stem));
                        std::fs::write(&out_path, &ciphertext)?;
                    }
                } else if input.contains('=') {
                    let (key, val) = input.split_once('=').unwrap();
                    let filename = age::var_to_filename(key);
                    let ciphertext = age::encrypt(val.as_bytes(), recipient_box.as_ref())?;
                    let out_path = Path::new(&output_dir).join(&filename);
                    std::fs::write(&out_path, &ciphertext)?;
                } else {
                    return Err(eyre::eyre!(
                        "Input '{}' is not an existing file and not a KEY=VAL pair",
                        input
                    ));
                }
            }
            Ok(())
        }
        None => Err(eyre::eyre!(
            "No action specified. Use 'encrypt' or 'decrypt' subcommand."
        )),
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    setup_logging()?;

    // Handle subcommands first
    if let Some(command) = cli.command {
        match command {
            Commands::Age {
                identity,
                recipient,
                keygen,
                public_key,
                action,
            } => {
                return handle_age_command(identity, recipient, keygen, public_key, action);
            }
        }
    }

    info!("Starting manifest generation");

    debug!("Parsed CLI arguments: {:?}", cli);

    let (manifest_spec, config_path) = ManifestSpec::load_from_standard_locations(cli.config.clone())?;
    debug!("Loaded manifest spec: {:?}", manifest_spec);
    debug!("Config path: {:?}", config_path);

    let repo_root = config::discover_repo_root(&config_path).unwrap_or_else(|_| PathBuf::from(&cli.path));
    debug!("Resolved repo root: {:?}", repo_root);

    ensure_manifest_functions().wrap_err("Failed to ensure manifest function files")?;

    let complete = !cli.any_section_specified();
    debug!("Complete mode = {}", complete);

    let mut sections: Vec<ManifestType> = Vec::new();

    if (complete || !cli.link.is_empty())
        && (!manifest_spec.link.items.is_empty() || manifest_spec.link.recursive || !manifest_spec.link.dirs.is_empty())
    {
        let lines = linkspec_to_vec(&manifest_spec.link, &repo_root, &cli)?;
        let filtered = fuzzy(lines).include(&cli.link);
        debug!("Adding Link section with {} lines", filtered.len());
        sections.push(ManifestType::Link(sorted_vec(&filtered)));
    }

    if complete || !cli.ppa.is_empty() {
        let ppa_items = fuzzy(manifest_spec.ppa.items.clone()).include(&cli.ppa);
        if !ppa_items.is_empty() {
            debug!("Adding Ppa section with {} items", ppa_items.len());
            sections.push(ManifestType::Ppa(sorted_vec(&ppa_items)));
        }
    }

    if cli.pkgmgr == "deb" && (complete || !cli.apt.is_empty()) {
        let merged = merge_pkg_apt(&manifest_spec);
        let apt_items = fuzzy(merged).include(&cli.apt);
        if !apt_items.is_empty() {
            debug!("Adding Apt section with {} merged items", apt_items.len());
            sections.push(ManifestType::Apt(sorted_vec(&apt_items)));
        }
    } else if cli.pkgmgr == "rpm" && (complete || !cli.dnf.is_empty()) {
        let merged = merge_pkg_dnf(&manifest_spec);
        let dnf_items = fuzzy(merged).include(&cli.dnf);
        if !dnf_items.is_empty() {
            debug!("Adding Dnf section with {} merged items", dnf_items.len());
            sections.push(ManifestType::Dnf(sorted_vec(&dnf_items)));
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

    if complete || !cli.uv_tool.is_empty() {
        let uv_tool_items = fuzzy(manifest_spec.uv_tool.items.clone()).include(&cli.uv_tool);
        if !uv_tool_items.is_empty() {
            debug!("Adding UVTool section with {} items", uv_tool_items.len());
            sections.push(ManifestType::UVTool(sorted_vec(&uv_tool_items)));
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
        let github_items: HashMap<String, RepoSpec> = fuzzy(manifest_spec.github.items.clone()).include(&cli.github);
        if !github_items.is_empty() {
            debug!("Adding Github section with {} repos", github_items.len());
            sections.push(ManifestType::Github(
                github_items,
                manifest_spec.github.repopath.clone(),
            ));
        }
    }

    if complete || !cli.git_crypt.is_empty() {
        let gitcrypt_items: HashMap<String, RepoSpec> =
            fuzzy(manifest_spec.git_crypt.items.clone()).include(&cli.git_crypt);
        if !gitcrypt_items.is_empty() {
            debug!("Adding GitCrypt section with {} repos", gitcrypt_items.len());
            sections.push(ManifestType::GitCrypt(
                gitcrypt_items,
                manifest_spec.git_crypt.repopath.clone(),
            ));
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

    fn make_cli(home: &str) -> Cli {
        Cli {
            config: None,
            home: home.to_string(),
            pkgmgr: "deb".to_string(),
            link: vec![],
            ppa: vec![],
            apt: vec![],
            dnf: vec![],
            npm: vec![],
            pip3: vec![],
            pipx: vec![],
            uv_tool: vec![],
            flatpak: vec![],
            cargo: vec![],
            github: vec![],
            git_crypt: vec![],
            script: vec![],
            path: ".".to_string(),
            command: None,
        }
    }

    #[test]
    fn test_linkspec_to_vec_dirs_produces_one_line() {
        let tmp = TempDir::new().unwrap();
        let repo_root = tmp.path().to_path_buf();
        let src_dir = repo_root.join("HOME/.claude/skills");
        fs::create_dir_all(&src_dir).unwrap();
        fs::write(src_dir.join("file1.txt"), "a").unwrap();
        fs::write(src_dir.join("file2.txt"), "b").unwrap();

        let mut dirs = HashMap::new();
        dirs.insert("HOME/.claude/skills".to_string(), "$HOME/.claude/skills".to_string());
        let spec = config::LinkSpec {
            recursive: false,
            dirs,
            items: HashMap::new(),
        };

        let cli = make_cli("/test/home");
        let lines = linkspec_to_vec(&spec, &repo_root, &cli).unwrap();

        // Must be exactly 1 line - the directory pair, not one per file inside
        assert_eq!(lines.len(), 1);
        assert!(lines[0].ends_with("/test/home/.claude/skills"));
    }

    #[test]
    fn test_linkspec_to_vec_dirs_only_produces_output() {
        // A LinkSpec with only dirs (no items, recursive: false) must still
        // produce output - guards against the gating-condition bug.
        let tmp = TempDir::new().unwrap();
        let repo_root = tmp.path().to_path_buf();

        let mut dirs = HashMap::new();
        dirs.insert("some/dir".to_string(), "$HOME/some/dir".to_string());
        let spec = config::LinkSpec {
            recursive: false,
            dirs,
            items: HashMap::new(),
        };

        let cli = make_cli("/test/home");
        let lines = linkspec_to_vec(&spec, &repo_root, &cli).unwrap();

        assert_eq!(lines.len(), 1);
        assert!(lines[0].ends_with("/test/home/some/dir"));
    }

    fn setup_test_env() -> (TempDir, String) {
        let temp_home = TempDir::new().unwrap();
        let home_path = temp_home.path().to_string_lossy().to_string();
        (temp_home, home_path)
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
    fn test_ensure_manifest_functions_writes_embedded_when_absent() {
        let (_temp_home, home_path) = setup_test_env();

        let written = ensure_manifest_functions_with_home(Some(&home_path)).unwrap();

        // Both embedded helpers are delivered on a clean data dir.
        assert!(written.contains(&"linker.sh".to_string()));
        assert!(written.contains(&"latest.sh".to_string()));
        assert!(file_exists_in_manifest_dir(&home_path, "linker.sh"));
        assert!(file_exists_in_manifest_dir(&home_path, "latest.sh"));
        // Delivered content is the binary's embedded source of truth.
        assert_eq!(
            read_file_from_manifest_dir(&home_path, "linker.sh"),
            crate::manifest::LINKER
        );
        assert_eq!(
            read_file_from_manifest_dir(&home_path, "latest.sh"),
            crate::manifest::LATEST
        );
    }

    #[test]
    fn test_ensure_manifest_functions_refreshes_when_content_differs() {
        let (_temp_home, home_path) = setup_test_env();

        // Pre-seed a stale linker.sh (mimics the real-world Jun-2025 orphan).
        let manifest_dir = manifest_dir_path(&home_path);
        fs::create_dir_all(&manifest_dir).unwrap();
        fs::write(format!("{}/linker.sh", manifest_dir), "stale() { :; }\n").unwrap();

        let written = ensure_manifest_functions_with_home(Some(&home_path)).unwrap();

        // The drifted helper is rewritten to the embedded copy.
        assert!(written.contains(&"linker.sh".to_string()));
        assert_eq!(
            read_file_from_manifest_dir(&home_path, "linker.sh"),
            crate::manifest::LINKER
        );
    }

    #[test]
    fn test_ensure_manifest_functions_leaves_identical_untouched() {
        let (_temp_home, home_path) = setup_test_env();

        // First call delivers everything.
        ensure_manifest_functions_with_home(Some(&home_path)).unwrap();
        // Second call: nothing drifted, so nothing is rewritten.
        let written = ensure_manifest_functions_with_home(Some(&home_path)).unwrap();

        assert!(written.is_empty());
        assert_eq!(
            read_file_from_manifest_dir(&home_path, "linker.sh"),
            crate::manifest::LINKER
        );
        assert_eq!(
            read_file_from_manifest_dir(&home_path, "latest.sh"),
            crate::manifest::LATEST
        );
    }
}

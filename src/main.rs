// src/main.rs

mod age;
mod cli;
mod config;
mod fuzzy;
mod manifest;

use crate::cli::{AgeAction, Cli, Commands, DecryptFormat, SecretsAction};
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
use std::io::{IsTerminal, Write};
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
                        // Boundary-checked so sibling files sharing the dir's name prefix
                        // (e.g. "voice-scrub-map.yml" next to a "voice" dir) are not skipped.
                        let rel_from_cwd = path.strip_prefix(cwd).unwrap_or(path);
                        let rel_str = rel_from_cwd.to_string_lossy();
                        if spec
                            .dirs
                            .keys()
                            .any(|d| rel_str.as_ref() == d.as_str() || rel_str.starts_with(&format!("{}/", d)))
                        {
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

/// Resolve the output directory for `--name`/`--paste` new-mode paths.
///
/// Resolution order (highest priority first):
/// 1. `-o DIR` if given explicitly.
/// 2. `secrets-store` from the loaded `ManifestSpec` (tilde-expanded).
/// 3. Neither set -> `eyre!` with a fix-it message.
///
/// Config is loaded **lazily** here - only when `-o` was not provided - so a
/// missing or malformed `manifest.yml` cannot affect `--keygen`, `--public-key`,
/// `decrypt`, or any path where `-o` was given explicitly. This matches the
/// constraint from CRITICAL #3 in the design doc (do not reorder the global load).
///
/// A `secrets-store` path that does not exist yet is passed through; the
/// refuse-to-invent guard in `encrypt_named` will reject it at write time.
fn resolve_new_output_dir(output_dir: Option<&PathBuf>) -> Result<PathBuf> {
    debug!("resolve_new_output_dir: output_dir={:?}", output_dir);
    if let Some(dir) = output_dir {
        debug!("resolve_new_output_dir: using explicit -o dir={}", dir.display());
        return Ok(dir.clone());
    }

    // Lazy config load: discover and parse manifest.yml only now.
    // Mirror how main.rs:~439 calls load_from_standard_locations(None) for the
    // global load path, but swallow load errors as a friendly "no output dir" message
    // so that a missing manifest.yml cannot break the other age subpaths.
    match ManifestSpec::load_from_standard_locations(None) {
        Ok((spec, config_path)) => {
            debug!("resolve_new_output_dir: loaded config from {:?}", config_path);
            if let Some(store) = spec.secrets_store {
                debug!("resolve_new_output_dir: using secrets-store={}", store.display());
                return Ok(store);
            }
        }
        Err(e) => {
            debug!(
                "resolve_new_output_dir: config load failed ({}); no secrets-store available",
                e
            );
        }
    }

    Err(eyre::eyre!(
        "no output directory: pass -o DIR or set secrets-store in manifest.yml"
    ))
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
        Some(AgeAction::Encrypt {
            inputs,
            name,
            paste,
            force,
            clear_clipboard,
            output_dir,
        }) => {
            debug!(
                "handle_age_command: encrypt inputs.len={} name={:?} paste={:?} force={} clear_clipboard={} output_dir={:?}",
                inputs.len(),
                name,
                paste,
                force,
                clear_clipboard,
                output_dir
            );

            // Manual exclusivity validation. The clap `encrypt-input` ArgGroup is a
            // backstop, not the sole gate: clap is historically fragile expressing
            // mutual exclusion between a Vec<String> positional and flags. Reject
            // every disallowed combination explicitly with a clear error.
            if name.is_some() && paste.is_some() {
                return Err(eyre::eyre!(
                    "--name and --paste are mutually exclusive; choose one input source"
                ));
            }
            if name.is_some() && !inputs.is_empty() {
                return Err(eyre::eyre!(
                    "--name cannot be combined with positional inputs; choose one input source"
                ));
            }
            if paste.is_some() && !inputs.is_empty() {
                return Err(eyre::eyre!(
                    "--paste cannot be combined with positional inputs; choose one input source"
                ));
            }
            // --clear-clipboard only makes sense with --paste (the only mode that reads
            // the clipboard). Reject it up front when used with --name, KEY=VAL, file,
            // or "-" so the user gets a clear diagnostic rather than a silent no-op.
            if clear_clipboard && paste.is_none() {
                return Err(eyre::eyre!("--clear-clipboard is only valid with --paste"));
            }

            let recipient_box = age::resolve_recipient(recipient.as_deref(), identity.as_deref())?;
            let identity_path = identity.as_deref().map(Path::new);

            // --name: read secret from stdin, write <name>.age atomically.
            if let Some(ref name) = name {
                debug!("handle_age_command: --name path name={}", name);

                // Guard: if stdin is an interactive TTY there is nothing to read.
                if std::io::stdin().is_terminal() {
                    return Err(eyre::eyre!("stdin is a TTY; pipe a value into --name, or use --paste"));
                }

                // Resolve output dir: -o DIR > secrets-store (lazy config load) > error.
                // Config is loaded lazily here - only when -o was not given - so that
                // missing/malformed manifest.yml cannot break --keygen, --public-key,
                // decrypt, or -o-explicit encrypt paths. The global config load at
                // main.rs:~439 is not reordered.
                let out_dir = resolve_new_output_dir(output_dir.as_ref())?;

                // BYTE-EXACT: read stdin and encrypt without stripping a trailing
                // newline. A file secret (ssh key, PEM cert) ends in `\n` and must
                // round-trip exactly; the env lane still strips at emit. (Previously
                // this path stripped here and corrupted newline-terminated files.)
                let written = age::encrypt_named_from_reader(
                    name,
                    &mut std::io::stdin().lock(),
                    recipient_box.as_ref(),
                    identity_path,
                    &out_dir,
                    force,
                )?;
                println!("encrypted: {}", written.display());
                return Ok(());
            }

            // --paste: read secret from the clipboard, then the same path as --name.
            if let Some(ref name) = paste {
                debug!(
                    "handle_age_command: --paste path name={} clear_clipboard={}",
                    name, clear_clipboard
                );

                // Read the secret read-only from the system clipboard. read_clipboard
                // already strips a single trailing newline and errors on empty.
                let plaintext = age::read_clipboard()?;

                // Resolve output dir: -o DIR > secrets-store (lazy config load) > error.
                let out_dir = resolve_new_output_dir(output_dir.as_ref())?;

                let written =
                    age::encrypt_named(name, &plaintext, recipient_box.as_ref(), identity_path, &out_dir, force)?;
                println!("encrypted: {}", written.display());

                // Opt-in clipboard clear: only after a successful, verified write.
                // A failure to clear is NON-FATAL (the file is already written and
                // verified). A bare warn! is invisible (log-file only per
                // src/main.rs:195-203), so we also eprintln! to stderr.
                if clear_clipboard {
                    debug!("handle_age_command: --clear-clipboard requested, clearing clipboard");
                    if let Err(e) = age::clear_clipboard() {
                        warn!("handle_age_command: clear_clipboard failed: {}", e);
                        eprintln!("WARNING: failed to clear the clipboard: {e}");
                    } else {
                        debug!("handle_age_command: clipboard cleared successfully");
                    }
                }

                return Ok(());
            }

            // Legacy positional modes use "." as the default output dir;
            // output_dir Option resolution for new --name/--paste modes is above.
            let legacy_output_dir = output_dir.unwrap_or_else(|| PathBuf::from("."));

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
                        let out_path = legacy_output_dir.join(format!("{}.age", stem));
                        std::fs::write(&out_path, &ciphertext)?;
                    }
                } else if input.contains('=') {
                    let (key, val) = input.split_once('=').unwrap();
                    // Route KEY=VAL through encrypt_named to gain: atomic temp-write +
                    // sync_all + round-trip verify + --force overwrite guard + validate_name
                    // + refuse-to-invent. Behavior change: an existing <key>.age without
                    // --force now errors instead of silently clobbering (deliberate per the
                    // output-behavior matrix). Output dir stays -o or "." (secrets-store is
                    // NOT applied here) to avoid silently relocating existing KEY=VAL scripts.
                    age::encrypt_named(
                        key,
                        val.as_bytes(),
                        recipient_box.as_ref(),
                        identity_path,
                        &legacy_output_dir,
                        force,
                    )?;
                } else {
                    return Err(eyre::eyre!(
                        "Input '{}' is not an existing file and not a KEY=VAL pair",
                        input
                    ));
                }
            }
            debug!("handle_age_command: encrypt completed for {} inputs", inputs.len());
            Ok(())
        }
        None => Err(eyre::eyre!(
            "No action specified. Use 'encrypt' or 'decrypt' subcommand."
        )),
    }
}

fn handle_secrets_command(config: Option<String>, action: SecretsAction) -> Result<()> {
    debug!("handle_secrets_command: action={:?}", action);
    match action {
        SecretsAction::Env { format } => secrets_env(config, &format),
        SecretsAction::Deploy { dry_run } => secrets_deploy(config, dry_run),
    }
}

/// `manifest secrets deploy [--dry-run]`: decrypt each `secrets.file` entry and
/// atomically write it to its destination path at `0600` (native Rust; the
/// decrypted bytes never pass through generated Bash or stdout).
///
/// `--dry-run` prints the planned `name -> path` for each entry and decrypts
/// NOTHING, writes NOTHING - it returns before any identity is resolved or any
/// ciphertext is read, so a corrupt/undecryptable entry cannot error under
/// `--dry-run`.
///
/// Batch semantics (non-dry-run): per-file atomic with report-and-continue - a
/// failed entry is reported to stderr + the log, the rest still deploy, and the
/// command exits non-zero if ANY entry failed. Entries are deployed in sorted
/// name order for deterministic, diffable output.
fn secrets_deploy(config: Option<String>, dry_run: bool) -> Result<()> {
    debug!("secrets_deploy: dry_run={}", dry_run);
    let (spec, config_path) = ManifestSpec::load_from_standard_locations(config)?;
    // Fail-closed: reject any name that is not a bare identifier BEFORE dry-run
    // print or any decrypt/write, so a key like `../evil` can never join a path
    // outside `.secrets/`. Same check the env lane and `encrypt_named` apply.
    age::validate_secret_names(spec.secrets.file.keys().map(String::as_str))?;
    let secrets_dir = resolve_secrets_dir(&spec, &config_path)?;

    // Deterministic order: a HashMap iterates in a different order every run.
    let mut entries: Vec<(String, String)> = spec
        .secrets
        .file
        .iter()
        .map(|(name, dest)| (name.clone(), dest.clone()))
        .collect();
    entries.sort();
    debug!(
        "secrets_deploy: entries={} secrets_dir={}",
        entries.len(),
        secrets_dir.display()
    );

    if dry_run {
        for (name, dest) in &entries {
            println!("{} -> {}", name, dest);
        }
        debug!("secrets_deploy: dry-run printed {} planned entries", entries.len());
        return Ok(());
    }

    let identity = age::resolve_identity(None)?;
    let report = age::deploy_secret_files(&entries, &secrets_dir, identity.as_ref());

    // Report each entry by name/path only - never a decrypted value.
    for name in &report.deployed {
        if let Some((_, dest)) = entries.iter().find(|(n, _)| n == name) {
            println!("deployed: {} -> {}", name, dest);
        }
    }
    for (name, err) in &report.failed {
        eprintln!("manifest: failed to deploy secret '{}': {}", name, err);
    }

    if !report.failed.is_empty() {
        return Err(eyre::eyre!(
            "{} of {} secret(s) failed to deploy",
            report.failed.len(),
            entries.len()
        ));
    }
    debug!("secrets_deploy: all {} entries deployed", report.deployed.len());
    Ok(())
}

/// Resolve the directory holding the `.age` ciphertext files for `secrets env`.
///
/// Order: an explicit `secrets-store` from the manifest (already tilde-expanded in
/// `load_manifest_spec`), else `<dir-of-manifest.yml>/.secrets` - the default
/// sibling store. Mirrors how `link:` resolves sources relative to the repo root:
/// the loaded manifest's own path gives the store location, so no store path needs
/// declaring for the common case.
fn resolve_secrets_dir(spec: &ManifestSpec, config_path: &Path) -> Result<PathBuf> {
    debug!("resolve_secrets_dir: config_path={}", config_path.display());
    if let Some(store) = &spec.secrets_store {
        debug!("resolve_secrets_dir: using secrets-store override {}", store.display());
        return Ok(store.clone());
    }
    let dir = config_path
        .parent()
        .ok_or_else(|| eyre::eyre!("manifest path {} has no parent directory", config_path.display()))?
        .join(".secrets");
    debug!("resolve_secrets_dir: default sibling store {}", dir.display());
    Ok(dir)
}

/// Build the interactive stderr banner for a command-level `secrets env` failure.
///
/// `declared` is the `secrets.env` allowlist size when the manifest parsed (so the
/// operator sees how many secrets dropped); `None` when the config itself never
/// parsed, in which case the count is genuinely unknown and is omitted rather than
/// fabricated. Pure and unit-testable; the caller gates the actual emission on an
/// interactive TTY.
fn secrets_env_banner(declared: Option<usize>) -> String {
    match declared {
        Some(n) => format!("manifest: secrets env failed, {} secrets not loaded", n),
        None => "manifest: secrets env failed, secrets not loaded".to_string(),
    }
}

/// Load the manifest and resolve the secrets store dir for `secrets env`.
///
/// Command-level failures (an unparseable/`deny_unknown_fields`-violating manifest,
/// a manifest path with no parent) surface here as `Err` BEFORE any identity is
/// touched or any byte is emitted - which is exactly what makes the "zero stdout on
/// command-level error" guarantee structural: the caller never reaches the emit
/// step when this returns `Err`.
fn secrets_env_context(config: Option<String>) -> Result<(ManifestSpec, PathBuf)> {
    debug!("secrets_env_context: config={:?}", config);
    let (spec, config_path) = ManifestSpec::load_from_standard_locations(config)?;
    // Fail-closed: reject any name that is not a bare identifier BEFORE resolving
    // the store or touching an identity, so a name like `../evil` can never join
    // a path outside `.secrets/`. Command-level error -> zero stdout in `secrets_env`.
    age::validate_secret_names(spec.secrets.env.iter().map(String::as_str))?;
    let secrets_dir = resolve_secrets_dir(&spec, &config_path)?;
    debug!(
        "secrets_env_context: env_count={} secrets_dir={}",
        spec.secrets.env.len(),
        secrets_dir.display()
    );
    Ok((spec, secrets_dir))
}

/// `manifest secrets env [-f export|env]`: emit the `secrets.env` allowlist.
///
/// Fully buffered: `render_secrets_env` builds the whole output string, and it is
/// printed in exactly ONE write on success, or NOTHING on a command-level error.
/// This is the load-bearing fail-soft discipline - a partial `NAME=val` line
/// already on stdout mutates the shell even if the process later exits non-zero
/// (proven in Phase 0), so the emit path must never stream.
///
/// Two failure classes, kept distinct:
/// - per-secret decrypt failure -> skipped inside `render_secrets_env` with a
///   `warn!`; the rest still emit (handled in age.rs, not here).
/// - command-level failure (bad config, no parent dir, unresolvable identity) ->
///   zero stdout, non-zero exit (the returned `Err`), plus a one-line stderr
///   banner gated on an interactive TTY so a lagging machine sees the drop instead
///   of silently losing every secret.
fn secrets_env(config: Option<String>, format: &DecryptFormat) -> Result<()> {
    debug!("secrets_env: format={:?}", format);

    // Track the declared allowlist size so the failure banner can name it. It is
    // only known once the manifest parses; a parse failure leaves it None.
    let mut declared: Option<usize> = None;
    let built = (|| -> Result<String> {
        let (spec, secrets_dir) = secrets_env_context(config)?;
        declared = Some(spec.secrets.env.len());
        let identity = age::resolve_identity(None)?;
        Ok(age::render_secrets_env(
            &spec.secrets.env,
            &secrets_dir,
            identity.as_ref(),
            format,
        ))
    })();

    match built {
        Ok(output) => {
            // Emit the complete buffer in one write. Nothing above reached stdout.
            print!("{}", output);
            debug!("secrets_env: emitted buffer len={}", output.len());
            Ok(())
        }
        Err(e) => {
            // Command-level failure: stdout stays empty (nothing was printed), the
            // Err drives a non-zero exit, and an interactive-only banner reports it.
            warn!("secrets_env: command-level failure: {}", e);
            if std::io::stderr().is_terminal() {
                eprintln!("{}", secrets_env_banner(declared));
            }
            Err(e)
        }
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    if let Err(e) = setup_logging() {
        // TTY-gate: `.zshenv` runs this on every shell incl. non-interactive
        // (scp/rsync/git-over-ssh); unguarded stderr there breaks those channels.
        // Mirrors the interactive gate on the `secrets env` failure banner.
        if std::io::stderr().is_terminal() {
            eprintln!("Warning: Failed to set up logging: {e}");
        }
    }

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
            Commands::Secrets { action } => {
                return handle_secrets_command(cli.config.clone(), action);
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

    // ---- Phase 6: --clear-clipboard validation ----

    #[test]
    fn test_clear_clipboard_without_paste_is_rejected() {
        // --clear-clipboard is only valid with --paste. Using it with --name must
        // produce a clear error before any clipboard or encryption operation runs.
        // We call handle_age_command directly with clear_clipboard=true, name=Some,
        // paste=None, and an identity that will fail (so we don't need a real key).
        // The validation runs before recipient resolution, so the error must mention
        // "--clear-clipboard is only valid with --paste".
        let result = handle_age_command(
            None,  // identity
            None,  // recipient
            false, // keygen
            false, // public_key
            Some(crate::cli::AgeAction::Encrypt {
                inputs: vec![],
                name: Some("MY_SECRET".to_string()),
                paste: None,
                force: false,
                clear_clipboard: true,
                output_dir: None,
            }),
        );
        assert!(
            result.is_err(),
            "expected error when --clear-clipboard used without --paste"
        );
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("--clear-clipboard is only valid with --paste"),
            "expected message about --paste, got: {}",
            msg
        );
    }

    #[test]
    fn test_clear_clipboard_without_paste_kv_is_rejected() {
        // --clear-clipboard with a KEY=VAL positional (no --paste) must also be rejected.
        let result = handle_age_command(
            None,
            None,
            false,
            false,
            Some(crate::cli::AgeAction::Encrypt {
                inputs: vec!["MY_KEY=my_value".to_string()],
                name: None,
                paste: None,
                force: false,
                clear_clipboard: true,
                output_dir: None,
            }),
        );
        assert!(
            result.is_err(),
            "expected error when --clear-clipboard used with KEY=VAL"
        );
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("--clear-clipboard is only valid with --paste"),
            "expected message about --paste, got: {}",
            msg
        );
    }

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

    #[test]
    fn test_linkspec_to_vec_skip_respects_path_boundary() {
        // A dirs: key of "a/b/voice" must not skip the sibling file
        // "a/b/voice-scrub-map.yml", which merely shares a name prefix.
        // It must still skip files actually under "a/b/voice/".
        let tmp = TempDir::new().unwrap();
        let repo_root = tmp.path().to_path_buf();
        let base = repo_root.join("a/b");
        let voice_dir = base.join("voice");
        fs::create_dir_all(&voice_dir).unwrap();
        fs::write(base.join("voice-scrub-map.yml"), "sibling").unwrap();
        fs::write(voice_dir.join("inner.txt"), "inside").unwrap();

        let mut items = HashMap::new();
        items.insert("a/b".to_string(), "$HOME/a/b".to_string());
        let mut dirs = HashMap::new();
        dirs.insert("a/b/voice".to_string(), "$HOME/a/b/voice".to_string());
        let spec = config::LinkSpec {
            recursive: true,
            dirs,
            items,
        };

        let cli = make_cli("/test/home");
        let lines = linkspec_to_vec(&spec, &repo_root, &cli).unwrap();

        assert!(
            lines.iter().any(|l| l.contains("voice-scrub-map.yml")),
            "sibling file sharing the dir's name prefix must be linked, got: {:?}",
            lines
        );
        assert!(
            !lines
                .iter()
                .any(|l| l.contains("voice/inner.txt") || l.contains("inner.txt")),
            "file under the dirs: entry must be skipped (covered by the dir symlink), got: {:?}",
            lines
        );
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

    // ---- Phase 2: secrets env dispatch helpers ----

    fn write_manifest(dir: &Path, content: &str) -> PathBuf {
        let path = dir.join("manifest.yml");
        fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn test_resolve_secrets_dir_defaults_to_sibling_dot_secrets() {
        // No secrets-store override -> `<dir-of-manifest.yml>/.secrets`.
        let spec = ManifestSpec::default();
        let config_path = Path::new("/some/repo/manifest.yml");
        let dir = resolve_secrets_dir(&spec, config_path).unwrap();
        assert_eq!(dir, PathBuf::from("/some/repo/.secrets"));
    }

    #[test]
    fn test_resolve_secrets_dir_uses_secrets_store_override() {
        // An explicit secrets-store wins over the default sibling dir.
        let spec = ManifestSpec {
            secrets_store: Some(PathBuf::from("/custom/store/.secrets")),
            ..Default::default()
        };
        let config_path = Path::new("/some/repo/manifest.yml");
        let dir = resolve_secrets_dir(&spec, config_path).unwrap();
        assert_eq!(dir, PathBuf::from("/custom/store/.secrets"));
    }

    #[test]
    fn test_secrets_env_banner_with_and_without_count() {
        assert_eq!(
            secrets_env_banner(Some(5)),
            "manifest: secrets env failed, 5 secrets not loaded"
        );
        // Config never parsed -> count unknown -> omit the number rather than lie.
        assert_eq!(
            secrets_env_banner(None),
            "manifest: secrets env failed, secrets not loaded"
        );
    }

    #[test]
    fn test_secrets_env_context_errors_on_unparseable_manifest() {
        // A `deny_unknown_fields` violation under `secrets:` is a command-level
        // failure that surfaces at config load - BEFORE any identity is resolved or
        // any byte emitted. This proves the abort happens ahead of the emit step.
        let tmp = TempDir::new().unwrap();
        let bad = write_manifest(tmp.path(), "secrets:\n  bogus: x\n");
        let result = secrets_env_context(Some(bad.to_string_lossy().into_owned()));
        assert!(result.is_err(), "unparseable manifest must be a command-level error");
    }

    #[test]
    fn test_secrets_env_command_level_failure_returns_err_no_stdout() {
        // secrets_env prints to stdout ONLY in its Ok arm. A command-level failure
        // returns Err (driving a non-zero exit) having emitted nothing to stdout;
        // asserting Err here is asserting the zero-stdout guarantee structurally.
        let tmp = TempDir::new().unwrap();
        let bad = write_manifest(tmp.path(), "secrets:\n  bogus: x\n");
        let result = secrets_env(Some(bad.to_string_lossy().into_owned()), &DecryptFormat::Export);
        assert!(result.is_err(), "command-level failure must return Err (non-zero exit)");
    }

    // ---- Phase 3: secrets deploy --dry-run ----

    #[test]
    fn test_secrets_deploy_dry_run_writes_and_decrypts_nothing() {
        // --dry-run must return Ok, create NO destination file, and decrypt
        // nothing. We point a file entry at a dest under a non-existent parent AND
        // provide NO backing .age (the store dir does not even exist), so if
        // dry-run tried to decrypt or write it would error or create the parent.
        // It does neither: it returns before resolving an identity or reading any
        // ciphertext.
        let tmp = TempDir::new().unwrap();
        let dest_dir = TempDir::new().unwrap();
        let dest = dest_dir.path().join("subdir").join("signing");

        let manifest = format!("secrets:\n  file:\n    work-signing-key: {}\n", dest.to_string_lossy());
        let path = write_manifest(tmp.path(), &manifest);

        let result = secrets_deploy(Some(path.to_string_lossy().into_owned()), true);
        assert!(result.is_ok(), "dry-run must succeed: {:?}", result.err());
        assert!(!dest.exists(), "dry-run must write no destination file");
        assert!(
            !dest.parent().unwrap().exists(),
            "dry-run must not create the parent directory"
        );
    }

    // ---- audit fix: non-bare secret names are rejected up-front (command-level) ----

    #[test]
    fn test_secrets_env_context_rejects_non_bare_name() {
        // A `secrets.env` entry that is not a bare identifier would `join` a
        // ciphertext path outside `.secrets/`. It must be rejected at config time,
        // BEFORE any identity/decrypt -> command-level error -> zero stdout in
        // `secrets_env`. Remove the `validate_secret_names` call and this fails.
        let tmp = TempDir::new().unwrap();
        let path = write_manifest(tmp.path(), "secrets:\n  env:\n    - ../../other-store/key\n");
        let result = secrets_env_context(Some(path.to_string_lossy().into_owned()));
        assert!(result.is_err(), "a non-bare secrets.env name must be rejected");
    }

    #[test]
    fn test_secrets_env_context_accepts_bare_names() {
        // Contrast case: valid bare identifiers must NOT be rejected by validation
        // (proves the reject test above bites on the name shape, not on load).
        let tmp = TempDir::new().unwrap();
        let path = write_manifest(tmp.path(), "secrets:\n  env:\n    - github-pat-home\n    - gh-token\n");
        let result = secrets_env_context(Some(path.to_string_lossy().into_owned()));
        assert!(result.is_ok(), "bare names must pass validation: {:?}", result.err());
    }

    #[test]
    fn test_secrets_deploy_rejects_non_bare_key_even_in_dry_run() {
        // A `secrets.file` KEY that is not a bare identifier must be rejected before
        // the dry-run print (and before any decrypt/write). dry_run=true needs no
        // identity, so this isolates the name-validation wiring on the deploy lane.
        let tmp = TempDir::new().unwrap();
        let path = write_manifest(tmp.path(), "secrets:\n  file:\n    ../evil: /tmp/pwned\n");
        let result = secrets_deploy(Some(path.to_string_lossy().into_owned()), true);
        assert!(
            result.is_err(),
            "a non-bare secrets.file key must be rejected even in dry-run"
        );
    }

    /// Run the embedded linker.sh against (src, dst) in bash, the same way
    /// `manifest -l '*' | bash` does at deploy time.
    fn run_linker(tmp: &TempDir, src: &std::path::Path, dst: &std::path::Path) -> std::process::Output {
        let script = tmp.path().join("linker.sh");
        fs::write(&script, crate::manifest::LINKER).unwrap();
        std::process::Command::new("bash")
            .arg("-c")
            .arg(format!(
                "source '{}'; linker '{}' '{}'",
                script.display(),
                src.display(),
                dst.display()
            ))
            .output()
            .unwrap()
    }

    /// A path is present if it exists as a link, even a self-referential one:
    /// `exists()` follows the link and reports the directory it lands on.
    fn link_present(path: &std::path::Path) -> bool {
        fs::symlink_metadata(path).is_ok()
    }

    #[test]
    fn test_linker_leaves_a_correct_dir_symlink_alone() {
        // Steady state for a `link.dirs` entry: dst is already a symlink to src.
        // `-f` is false for a directory symlink, so the pre-fix "already linked"
        // test missed this and re-ran `ln -s`, nesting src/<name> -> src inside
        // the source tree on every deploy.
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("repo/HOME/Claude/writing/voice");
        let dst = tmp.path().join("home/Claude/writing/voice");
        fs::create_dir_all(&src).unwrap();
        fs::create_dir_all(dst.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&src, &dst).unwrap();

        let out = run_linker(&tmp, &src, &dst);

        assert!(
            out.status.success(),
            "linker failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(
            !link_present(&src.join("voice")),
            "linker nested a self-referential loop inside the source directory"
        );
        assert_eq!(fs::read_link(&dst).unwrap(), src);
    }

    #[test]
    fn test_linker_replaces_a_stale_dir_symlink_in_place() {
        // A directory link pointing at the wrong target is repointed, not nested.
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("repo/voice");
        let stale = tmp.path().join("repo/voice-old");
        let dst = tmp.path().join("home/voice");
        fs::create_dir_all(&src).unwrap();
        fs::create_dir_all(&stale).unwrap();
        fs::create_dir_all(dst.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&stale, &dst).unwrap();

        let out = run_linker(&tmp, &src, &dst);

        assert!(
            out.status.success(),
            "linker failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(fs::read_link(&dst).unwrap(), src);
        assert!(!link_present(&src.join("voice")), "nested into the new target");
        assert!(!link_present(&stale.join("voice")), "nested into the stale target");
    }

    #[test]
    fn test_linker_refuses_a_real_directory_at_dest() {
        // Pre-existing guard: a REAL directory at dst is a hard error, never a nest.
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("repo/voice");
        let dst = tmp.path().join("home/voice");
        fs::create_dir_all(&src).unwrap();
        fs::create_dir_all(&dst).unwrap();

        let out = run_linker(&tmp, &src, &dst);

        assert!(!out.status.success(), "a real directory at dst must be refused");
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("real directory"),
            "stderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(
            !link_present(&dst.join("voice")),
            "linker nested inside a real directory"
        );
    }
}

# manifest

A Rust CLI that reads a YAML manifest describing system configuration (symlinks, packages, repos, scripts) and generates a Bash script to install everything.

## Quick Reference

```
otto ci                        # Full CI: lint + check + test
otto test                      # Run tests
otto check                     # Compile + clippy + format check
cargo test test_name           # Run a single test
cargo test --all-features      # Run all tests
otto build                     # Release build
cargo install --path .         # Install locally
```

## Architecture

```
src/
  main.rs       # CLI entry point, section assembly, age subcommand dispatch
  cli.rs        # Clap derive structs (Cli, Commands, AgeAction)
  config.rs     # YAML deserialization into ManifestSpec, config discovery, repo root detection
  fuzzy.rs      # Fuzzy matching trait (Fuzz) for filtering items by glob/regex/prefix/contains
  manifest.rs   # ManifestType enum, Bash script rendering (heredoc, continue, github, gitcrypt)
  age.rs        # age encryption/decryption (encrypt, decrypt, keygen, public-key)
  scripts/      # Shell functions (linker.sh, latest.sh) embedded via include_str!
```

**Flow:** `manifest.yml` -> `ManifestSpec` (serde) -> filter sections via CLI flags + fuzzy matching -> `ManifestType` variants -> render each to Bash -> concatenate into final script -> stdout.

The output is meant to be piped to bash: `manifest | bash` or `manifest --link 'pattern' | bash`.

### Subcommands

- Default (no subcommand): generate Bash script from manifest
- `age encrypt`: encrypt files or KEY=VAL pairs using age
- `age decrypt`: decrypt .age files, output as export or env format
- `age --keygen`: generate new age identity
- `age --public-key`: show public key from identity file

## Config Discovery

Config file search order:
1. `~/.config/manifest/manifest.yml`
2. `./HOME/.config/manifest/manifest.yml`
3. `./manifest.yml`

Repo root discovery: follows config symlink back to find parent of `HOME/` directory.

## Build & Test

- **CI:** `otto ci` runs lint, check, test in parallel
- **Lint:** `whitespace -r`
- **Check:** `cargo check --all-targets --all-features && cargo clippy -- -D warnings && cargo fmt --all --check`
- **Test:** `cargo test --all-features`
- **Coverage:** `otto cov` (not part of CI, manual)
- **Version:** driven by `git describe --tags --always` via `build.rs`

## Coding Conventions

- Edition 2024, error handling via `eyre`
- Logging to `~/.local/share/manifest/logs/manifest.log`
- Version from `GIT_DESCRIBE` env var set by `build.rs`
- Tests are inline `#[cfg(test)] mod tests` in each module
- Uses `tempfile` for filesystem tests
- Fuzzy matching uses cascading match types: Exact -> IgnoreCase -> Prefix -> Contains

## Key Files

| File | Purpose |
|------|---------|
| `Cargo.toml` | Dependencies, version (0.1.5), edition 2024 |
| `build.rs` | Sets GIT_DESCRIBE for version reporting |
| `.otto.yml` | CI pipeline config (lint, check, test, cov, build) |
| `manifest.yml` | Real manifest config (used in integration tests) |
| `test/manifest.yml` | Test fixture manifest with complex nested specs |
| `src/scripts/linker.sh` | Shell function for symlink creation (embedded) |
| `src/scripts/latest.sh` | Shell function for script execution (embedded) |
| `docs/design/` | Design documents for features |

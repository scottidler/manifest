# Design Document: XDG Config Location and Symlink-Based Repo Discovery

**Author:** Scott Idler
**Date:** 2026-03-20
**Status:** Implemented
**Review Passes Completed:** 5/5

## Summary

Move `manifest.yml` from the root of `scottidler/...` into `HOME/.config/manifest/manifest.yml` so it gets symlinked into `~/.config/manifest/manifest.yml` like every other dotfile. Update `manifest` to resolve the symlink of its own config file to discover the repo root automatically, eliminating the need for an explicit path argument or config field.

## Problem Statement

### Background

The `scottidler/...` repo (checked out at `~/...`) is a dotfiles repo managed by the `manifest` tool (Rust binary from `scottidler/manifest`). The repo contains a `HOME/` directory whose contents are recursively symlinked into `$HOME`. Everything under `HOME/.config/` follows XDG conventions and lands in `~/.config/` via symlinks.

The `manifest` tool generates a bash script from `manifest.yml` that provisions the system: creating symlinks, installing packages, cloning repos, running scripts.

### Problem

`manifest.yml` currently lives at the repo root (`~/...manifest.yml`), breaking the pattern every other config file follows. All other configs live under `HOME/.config/<tool>/` and get symlinked into place. The manifest config is the sole exception.

Today, `manifest` requires the user to either:
1. Run from the repo root directory (so `./manifest.yml` and `./HOME/` are found via `cli.path` defaulting to `.`), or
2. Pass an explicit `-C` path to the config and a positional `PATH` arg pointing to the repo

This coupling between working directory and repo location is unnecessary and fragile.

Additionally, there is a latent bug in the current code: `-C/--config` defaults to `"manifest.yml"`, which means `load_from_standard_locations()` is always called with `Some("manifest.yml")` and never reaches `find_config_file()`. The XDG discovery path (`$HOME/.config/manifest/manifest.yml`) that already exists in code is dead code today.

The `ensure_manifest_functions()` function also reads from a `bin/` directory relative to the current working directory, which has the same CWD-coupling problem.

### Goals

- `manifest.yml` lives at `HOME/.config/manifest/manifest.yml` in the dotfiles repo, symlinked to `~/.config/manifest/manifest.yml` - consistent with every other config
- `manifest` can be run from any directory with no arguments and Just Works
- The repo root (containing `HOME/`) is discovered automatically by resolving the config file's symlink
- Legacy Python artifacts (`manifest.py`, `pyproject.toml`, `poetry.lock`) are cleaned up from `scottidler/...`
- `scottidler/dotfiles` is evaluated for cleanup or archival

### Non-Goals

- Changing the `manifest.yml` schema (beyond removing fields if any become unnecessary)
- Changing how `manifest` generates or executes bash scripts
- Supporting multiple dotfiles repos simultaneously
- Migrating from `scottidler/...` to `scottidler/dotfiles` (separate effort if desired)

## Proposed Solution

### Overview

The core idea is a two-line algorithm:

1. Find config at `~/.config/manifest/manifest.yml` (XDG default) or via `-C` override
2. Resolve the config file's symlink to its real path, walk up to find `HOME/` in the ancestry, and use its parent as the repo root

The symlink itself encodes the repo location - no new config fields needed.

### Repo Discovery Algorithm

```
config path:  ~/.config/manifest/manifest.yml
     |
     | fs::canonicalize() (resolve symlink)
     v
real path:    ~/...HOME/.config/manifest/manifest.yml
     |
     | walk ancestors, find component named "HOME"
     v
repo root:    ~/...
```

In Rust:

```rust
fn discover_repo_root(config_path: &Path) -> Result<PathBuf> {
    let real_path = fs::canonicalize(config_path)?;

    for ancestor in real_path.ancestors() {
        if ancestor.file_name() == Some(OsStr::new("HOME")) {
            return ancestor.parent()
                .map(|p| p.to_path_buf())
                .ok_or_else(|| eyre!("HOME directory has no parent"));
        }
    }

    // Fallback: config is not a symlink (or not inside a HOME/ tree).
    // Check if a HOME/ directory exists as a sibling or child relative to the config.
    // This covers bootstrap (running from repo root with ./HOME/.config/manifest/manifest.yml)
    // and the case where -C points at a non-symlinked file in the repo.
    let config_dir = real_path.parent().ok_or_else(|| eyre!("config has no parent"))?;
    for ancestor in config_dir.ancestors() {
        if ancestor.join("HOME").is_dir() {
            return Ok(ancestor.to_path_buf());
        }
    }

    Err(eyre!("could not discover repo root: no HOME/ directory found in ancestry of {:?}", real_path))
}
```

### Changes to `scottidler/manifest`

#### 1. Config resolution (src/config.rs)

`find_config_file()` already checks `$HOME/.config/manifest/manifest.yml` first. No change needed there.

`load_from_standard_locations()` gains a return of the resolved config path so callers can use it for repo discovery:

```rust
pub fn load_from_standard_locations(config_path: Option<String>) -> Result<(Self, PathBuf)> {
    let config_file = match config_path {
        Some(path) => PathBuf::from(path),
        None => Self::find_config_file()?,
    };

    let spec = if config_file.exists() {
        let file = std::fs::File::open(&config_file)?;
        load_manifest_spec(file)?
    } else {
        ManifestSpec::default()
    };

    Ok((spec, config_file))
}
```

#### 2. CLI changes (src/cli.rs)

- Remove the `default_value = "manifest.yml"` from `-C/--config` so it becomes `Option<String>`. This unblocks the existing `find_config_file()` XDG discovery that is currently dead code.
- The positional `PATH` argument remains as a manual override/fallback, used when repo discovery fails (e.g., non-symlinked config).

Note: clap does not support both `-c` and `-C` as short flags for the same argument. Since `-C` is already established, keep it. Users who want the lowercase experience can use `--config`.

#### 3. Main orchestration (src/main.rs)

Currently `cli.path` (defaulting to `.`) is passed into `linkspec_to_vec` as the working directory. After this change:

```rust
let (manifest_spec, config_path) = ManifestSpec::load_from_standard_locations(cli.config.clone())?;
let repo_root = discover_repo_root(&config_path)
    .unwrap_or_else(|_| PathBuf::from(&cli.path));
```

Then `repo_root` replaces `cli.path` in:
- `linkspec_to_vec()` (line 49: `let cwd = Path::new(&cli.path)`)
- `ensure_manifest_functions()` which reads `bin/` relative to CWD - should read `bin/` relative to `repo_root`

### Changes to `scottidler/...`

#### 1. Move manifest.yml

```
mv ~/...manifest.yml ~/...HOME/.config/manifest/manifest.yml
```

After `manifest` creates the symlinks, this file will appear at `~/.config/manifest/manifest.yml`.

#### 2. Bootstrap consideration

Chicken-and-egg: the first time on a fresh machine, symlinks don't exist yet. `manifest` needs to find the config before it can create the symlinks. Two options:

- **Option A (recommended):** `manifest` falls through to `./manifest.yml` (current directory) if the XDG path doesn't exist. Since the first run is from the repo root, `./HOME/.config/manifest/manifest.yml` can be tried as another candidate.
- **Option B:** The user passes `-C HOME/.config/manifest/manifest.yml` on first run.

Option A keeps the zero-arg experience. Updated candidate list:

```rust
fn find_config_file() -> Result<PathBuf> {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let candidates = vec![
        PathBuf::from(format!("{}/.config/manifest/manifest.yml", home)),
        PathBuf::from("./HOME/.config/manifest/manifest.yml"),
        PathBuf::from("./manifest.yml"),  // backward compat
    ];
    // ...
}
```

#### 3. Remove legacy Python artifacts

From `scottidler/...`:
- `manifest.py` - replaced by Rust binary
- `pyproject.toml` - Python project config for manifest.py
- `poetry.lock` - Python lockfile
- `.distutils.pkgs` in `HOME/` (referenced by manifest.py's distutils feature, not used by Rust version)

Verify first: `manifest.py` references in shell configs, aliases, or scripts. If `manifest` (the Rust binary) is already the only thing being invoked, safe to remove.

#### 4. Evaluate `scottidler/dotfiles`

Check if it has any unique content not in `scottidler/...`. If not, archive it on GitHub (Settings > Archive).

### Implementation Plan

**Phase 1: manifest Rust changes** (scottidler/manifest)
1. Make `-C/--config` an `Option<String>` (remove default), unblocking `find_config_file()` XDG discovery
2. Add `./HOME/.config/manifest/manifest.yml` to `find_config_file()` candidate list for bootstrap
3. Update `load_from_standard_locations()` to return the resolved config path alongside the spec
4. Add `discover_repo_root()` function - symlink resolution with `HOME/` ancestor walk, fallback to `HOME/`-sibling search
5. Update `main.rs`: derive `repo_root` from config path, pass it into `linkspec_to_vec()` and `ensure_manifest_functions()`
6. Add tests: symlink resolution, non-symlinked fallback, bootstrap path, missing `HOME/` error case
7. Release new version

**Phase 2: dotfiles repo changes** (scottidler/...)
1. Create `HOME/.config/manifest/` directory
2. `git mv manifest.yml HOME/.config/manifest/manifest.yml`
3. Run updated `manifest` from repo root to verify bootstrap path works (finds `./HOME/.config/manifest/manifest.yml`)
4. Run `manifest` again from a different directory to verify XDG symlink path works
5. Remove `manifest.py`, `pyproject.toml`, `poetry.lock`
6. Update README.md
7. Verify on both machines (desk, lappy)

**Phase 3: cleanup**
1. Diff `scottidler/dotfiles` against `scottidler/...` for unique content
2. Archive `scottidler/dotfiles` on GitHub if redundant

## Alternatives Considered

### Alternative 1: Add a `root:` or `path:` field to manifest.yml

- **Description:** A new top-level field like `root: ~/...` tells manifest where the repo is
- **Pros:** Explicit, simple to implement
- **Cons:** Redundant information - the symlink already encodes this relationship. Adds a field the user must keep in sync manually.
- **Why not chosen:** The symlink-tracing approach is zero-config and self-maintaining

### Alternative 2: Keep manifest.yml at repo root, only add -c flag

- **Description:** Just add the `-c` convenience alias, leave file where it is
- **Pros:** Minimal change
- **Cons:** Doesn't solve the consistency problem - manifest.yml remains the only config not under HOME/.config/
- **Why not chosen:** Misses the point of the refactor

### Alternative 3: Hardcode `~/...` as the repo path

- **Description:** Default repo root to `~/...`
- **Pros:** Simple
- **Cons:** Breaks for anyone who clones the repo elsewhere. Couples the tool to one user's conventions.
- **Why not chosen:** The symlink approach is general-purpose and works regardless of checkout location

## Technical Considerations

### Dependencies

- No new crate dependencies; `std::fs::canonicalize` handles symlink resolution
- `scottidler/manifest` changes must be released before the `scottidler/...` file move

### Performance

- `fs::canonicalize` is a single syscall; negligible overhead

### Security

- No new attack surface; symlink resolution uses the same filesystem permissions

### Testing Strategy

- Unit tests for `discover_repo_root()` with symlinked and non-symlinked paths using `tempfile::TempDir`
- Unit tests for updated `find_config_file()` candidate list
- Integration test: create a temp repo with `HOME/.config/manifest/manifest.yml`, symlink it, verify discovery
- Manual verification: move the file in `scottidler/...`, run `manifest`, confirm symlinks still work

### Rollout Plan

1. Ship manifest changes with backward compatibility (still searches `./manifest.yml`)
2. Move the file in `scottidler/...`
3. Verify on both machines (desk, lappy)
4. Remove legacy files in a separate commit

## Risks and Mitigations

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| Bootstrap on fresh machine fails | Medium | High | Add `./HOME/.config/manifest/manifest.yml` to candidate list |
| `fs::canonicalize` fails on broken symlink | Low | Medium | Fall back to `cli.path` if resolution fails |
| Other tools depend on `manifest.yml` at repo root | Low | Low | Grep for references before moving |
| Positional PATH arg behavior changes break scripts | Low | Medium | Keep PATH as fallback; log deprecation warning |

## Open Questions

- [x] Does anything else reference `~/...manifest.yml` by path? Only `manifest.py` (being removed) and `README.md` (being updated). No shell configs or scripts reference it.
- [ ] Should the `HOME` directory name be configurable or is it always `HOME`? Currently hardcoded as the key in the `link:` section of manifest.yml (`HOME: $HOME`). The discovery algorithm uses the directory name `HOME` from the real path. If someone renamed it, both places would need to change. Consider making `discover_repo_root()` take the directory name from the link spec key rather than hardcoding `"HOME"`.
- [ ] Should `scottidler/dotfiles` be archived or deleted?
- [ ] Should `ensure_manifest_functions()` also use the discovered repo root for its `bin/` directory, or does it intentionally read from the manifest repo's own `bin/`? (Currently reads `./bin/` relative to CWD, which in practice is the manifest repo when developing, but the dotfiles repo when running normally.)

## References

- `scottidler/...` repo (dotfiles): `~/repos/scottidler/.../`
- `scottidler/manifest` repo: `~/repos/scottidler/manifest/`
- Current config resolution: `src/config.rs:154-184`
- Current path usage: `src/main.rs:46-93` (`linkspec_to_vec`)

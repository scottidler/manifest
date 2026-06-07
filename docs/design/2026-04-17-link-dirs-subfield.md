# Design Document: `dirs` Sub-field for Directory-Level Symlinks

**Author:** Scott Idler
**Date:** 2026-04-17
**Status:** In Review
**Review Passes Completed:** 5/5

## Summary

Add a `dirs` sub-field to the `link:` section in `manifest.yml` that creates
directory-level symlinks rather than recursing into the source and creating
individual file symlinks. This allows a single entry like
`HOME/.claude/skills: $HOME/.claude/skills` to produce one directory symlink
at the destination, so any file created inside the target directory is
immediately tracked in the dotfiles repo without re-running manifest.

## Problem Statement

### Background

`scottidler/claude` is a dotfiles repo managed by `manifest`. It stores Claude
Code configuration under `HOME/.claude/` and symlinks everything into `~/.claude/`
via `link: { recursive: true, HOME: $HOME }`. Skills live under
`HOME/.claude/skills/<name>/` as directories containing `SKILL.md`, `run.sh`,
and other supporting files.

When a skill gains a new helper file (e.g., `run.sh` added after `SKILL.md`
already has a symlink), the new file has no symlink in `~/.claude/skills/<name>/`
until `manifest` is re-run. More critically, when Claude Code creates a new file
inside a skill directory, it writes to `~/.claude/skills/<name>/newfile`, which
is a real file in a real directory — not in the repo — so it is never tracked.

### Problem

The `link: recursive: true` mode walks the source tree with `WalkDir` and calls
`linker` for every file it finds (`is_file()` check). It produces file-level
symlinks. `recursive` is a single global flag on `LinkSpec`; there is no
per-item override.

The consequence: a skill directory like `~/.claude/skills/architect/` is a real
directory populated with individual file symlinks. Any new file written there
(by Claude Code or by hand) lands as an untracked real file, invisible to git.

### Goals

- Allow a single manifest entry to produce a **directory-level symlink**
  (`~/.claude/skills → repo/HOME/.claude/skills`) so the destination IS the
  repo directory.
- Keep existing `link: recursive: true` behaviour unchanged for all other
  entries.
- Stay within the `link:` section — no new top-level key in `manifest.yml`.

### Non-Goals

- Per-item override of the `recursive` flag for file-level entries.
- Support for `files:` sub-field (may follow separately if needed).
- Changing how external directory symlinks already in the source tree are
  handled (e.g., `defuddle → /path/to/external`).
- Supporting `dirs` inside `RepoSpec`/`GithubSpec`/`GitCryptSpec` link specs -
  `render_repo_links()` in `manifest.rs` is out of scope; only the top-level
  `link:` section is affected.

## Proposed Solution

### Overview

Extend `LinkSpec` with an optional `dirs` map. Entries in `dirs` are processed
non-recursively: each produces a single `linker src dst` call, which creates
`ln -s src dst`. The `recursive` flag and the flattened `items` map are
unaffected.

### Architecture

No new top-level section. `LinkSpec` gains one field:

```
LinkSpec
  recursive: bool          (existing)
  dirs: HashMap<String,String>   (new — directory-level symlinks)
  items: HashMap<String,String>  (existing — flattened, file-level)
```

`linkspec_to_vec` emits `dirs` entries as-is (source → dest path pair),
identical to the non-recursive `items` path. The generated bash script calls
`linker` with the directory path; `linker` already handles this correctly
because its final branch does `ln -s "$file" "$link"` when `$link` does not
exist as a regular file.

### Data Model

`config.rs` — `LinkSpec`:

```rust
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
pub struct LinkSpec {
    #[serde(default)]
    pub recursive: bool,
    #[serde(default)]
    pub dirs: HashMap<String, String>,   // new
    #[serde(flatten)]
    pub items: HashMap<String, String>,
}
```

`dirs` must be declared **before** `#[serde(flatten)]` so serde captures it as
a named field before the catch-all flatten consumes remaining keys.

### API Design

`manifest.yml` usage:

```yaml
link:
  recursive: true
  HOME: $HOME
  dirs:
    HOME/.claude/skills: $HOME/.claude/skills
```

`dirs` keys are relative paths from the repo root (same convention as `items`).
Values are destination paths supporting `$HOME` expansion and `~` expansion
(same as `items`).

Multiple entries are allowed:

```yaml
dirs:
  HOME/.claude/skills: $HOME/.claude/skills
  HOME/.config/nvim: $HOME/.config/nvim
```

### Implementation Plan

#### Phase 1: Extend `LinkSpec` in `config.rs`
**Model:** sonnet

- Add `dirs: HashMap<String, String>` field before `#[serde(flatten)]`.
- Add unit test: deserialise a `LinkSpec` YAML with both `dirs` and flat items;
  assert `dirs` is populated and does not bleed into `items`.
- Add unit test: serialise round-trips correctly.

#### Phase 2: Process `dirs` in `linkspec_to_vec` (`main.rs`)
**Model:** sonnet

- After existing recursive/non-recursive block, iterate `spec.dirs`.
- For each `(src, dst)`: resolve `cwd.join(src)`, expand `$HOME`, push
  `format!("{} {}", source_str, final_dst)` — identical to the non-recursive
  items path.
- `dirs` entries are appended unconditionally regardless of the `recursive` flag.
- `dirs` entries bypass `WalkDir` entirely — the source path is passed directly
  to `linker` without any file-system traversal, so directory symlinks in the
  source tree are handled correctly (unlike the recursive path where they are
  silently dropped by the `is_file()` filter).
- **Extend the gating condition** at `main.rs:347` from
  `(!manifest_spec.link.items.is_empty() || manifest_spec.link.recursive)` to
  also include `|| !manifest_spec.link.dirs.is_empty()` — otherwise a
  `LinkSpec` with only `dirs` (no `items`, `recursive: false`) will never call
  `linkspec_to_vec` and silently produce nothing.
- Add unit test: `linkspec_to_vec` with a `dirs` entry produces one line
  (the directory path pair), not one line per file inside.
- Add unit test: gating condition with only `dirs` populated (empty `items`,
  `recursive: false`) still invokes the link generation path.

#### Phase 2b: Guard real directories in `linker.sh`
**Model:** sonnet

`ln -s src existing_dir` does **not** fail when `existing_dir` is a real
directory - it silently nests a symlink inside it (e.g.
`~/.claude/skills/skills -> repo`). `linker.sh` currently only checks
`[ -f "$link" ]`, so it falls through to the `ln -s` call without any guard.

Add an explicit check before the final `ln -s`:

```bash
if [ -d "$link" ] && [ ! -L "$link" ]; then
  echo "WARNING: $link is a real directory, not a symlink. Remove it before manifest can create a directory symlink here." >&2
  exit 1
fi
```

This must fire before `ln -s` and must not interfere with the existing
`[ -f "$link" ]` backup path for regular files.

- Add unit test: `linker.sh` called with an existing real directory at the
  destination prints a warning to stderr and exits non-zero.

#### Phase 3: Update `scottidler/claude` manifest.yml and restructure
**Model:** sonnet

- Add `dirs` entry to `HOME/.config/manifest/manifest.yml` in the claude repo:
  `HOME/.claude/skills: $HOME/.claude/skills`.
- Delete the real `~/.claude/skills/` directory (which currently holds
  individual file symlinks) so `linker` can create the directory symlink in
  its place. The skill source files remain untouched in the repo at
  `HOME/.claude/skills/`.
- Run `manifest -l` and verify `~/.claude/skills` is now a symlink pointing to
  `~/repos/scottidler/claude/HOME/.claude/skills`.

## Alternatives Considered

### Alternative 1: Top-level `dirlinks:` section

- **Description:** New `DirLinkSpec` struct at the `ManifestSpec` level,
  processed alongside `link:`.
- **Pros:** Clean separation; `link:` stays unchanged.
- **Cons:** Adds a new top-level key; feels like a parallel concept that
  belongs under `link:`. Requires more boilerplate in `config.rs`, `manifest.rs`,
  and `main.rs`.
- **Why not chosen:** `dirs` as a sub-field keeps all symlinking logic co-located
  under `link:` and is a smaller API surface.

### Alternative 2: Per-item `recursive` override

- **Description:** Change `items` from `HashMap<String,String>` to
  `HashMap<String, LinkItemSpec>` where `LinkItemSpec` has `dst` and optional
  `recursive` field.
- **Pros:** Maximum flexibility.
- **Cons:** Breaking change to manifest.yml syntax; all existing entries need
  updating; far more complex deserialisation.
- **Why not chosen:** Overkill for the use case; `dirs` is explicit and obvious.

### Alternative 3: Automate manifest re-runs via git hook

- **Description:** Add a post-commit hook in `scottidler/claude` that re-runs
  `manifest -l` whenever skill directories change.
- **Pros:** No manifest code changes.
- **Cons:** Still creates file-level symlinks; new files written by Claude Code
  between commits are untracked. Doesn't solve the root problem.
- **Why not chosen:** Works around the symptom, not the cause.

## Technical Considerations

### Dependencies

No new dependencies. `WalkDir` is not used for `dirs` entries.

### Performance

Negligible. `dirs` entries skip the `WalkDir` traversal entirely.

### Security

No new surface. `ln -s` behaviour is identical to the existing linker path.

### Testing Strategy

- Unit test in `config.rs`: `dirs` deserialises separately from flattened `items`.
- Unit test in `main.rs`: `linkspec_to_vec` emits exactly one line per `dirs`
  entry (the directory pair), not one per file within.
- Integration: run manifest against a temp directory tree; assert destination
  is a symlink, not a real directory.

### Rollout Plan

1. Implement and test in `scottidler/manifest`.
2. Bump version, install updated binary.
3. Update `HOME/.config/manifest/manifest.yml` in `scottidler/claude`.
4. Remove stale file-level symlinks from `~/.claude/skills/`.
5. Run `manifest -l` to create the directory symlink.

## Risks and Mitigations

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| `dirs` key bleeds into flattened `items` | Low | High | Declare `dirs` before `#[serde(flatten)]`; unit test asserts no bleed |
| linker silently nests symlink inside existing real directory | High | High | `ln -s src existing_dir` does NOT fail - it creates `existing_dir/src -> ...`. `linker.sh` must explicitly check `[ -d "$link" ] && [ ! -L "$link" ]` and abort with a warning. Addressed in Phase 2b. |
| Existing manifest.ymls with a `dirs:` symlink entry break | Low | Med | `dirs` becomes a reserved key after this change. A user with `dirs: $HOME/.dirs` will get a fatal Serde parse error. Documented in code (warning comment above the field in `config.rs`), in `README.md`, and in release notes; no escape hatch planned. The proper fix (custom Deserialize with type-based discrimination) is deferred. |
| `dirs`-only `LinkSpec` silently skipped | Med | Med | Gating condition at `main.rs:347` must be extended; addressed in Phase 2. |

## Open Questions

- [ ] Should `dirs` entries also support `$HOME` / `~` expansion for the
      source path, or only the destination? (Current `items` only expands dst.)
- [x] Should manifest emit a visible warning when a `dirs` destination already
      exists as a real directory (not a symlink)? **Resolved: yes, required.**
      `linker.sh` must abort with a warning rather than silently nesting.
      See Phase 2b.
- [ ] `dirs` semantically means "symlink this directory" but linker doesn't
      validate that the source is a directory. Acceptable for now — the name
      is a hint, not a constraint.

## References

- `src/config.rs` — `LinkSpec` definition
- `src/main.rs` — `linkspec_to_vec`
- `src/scripts/linker.sh` — symlink creation logic
- Prior design: `docs/design/2026-03-20-xdg-config-and-repo-discovery.md`

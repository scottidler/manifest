# Implementation Notes: Secrets Folder Split

Design doc: `docs/design/2026-10-23-secrets-folder-split.md`

## Phase 1: manifest: directory-driven lanes

### Design decisions
- Lane logic is pure data in `age.rs`: `enumerate_env_lane` returns `EnvLane { names, warnings }`, and `check_lanes`, `find_strays`, `check_lane_root` return `Result` or warning strings. `main.rs` owns the log and stderr through `emit_lane_warning`. This follows the repo's return-data-not-side-effects convention.
- `src/age.rs:check_lanes` is one shared chokepoint for both lanes. It checks both lane roots for symlinks (through `check_lane_root`) and rejects a stem present in both `.env/` and `.file/`. `secrets env` and `secrets deploy` both call it, so the two lanes cannot drift on the collision or symlink-root rules.
- `src/age.rs:enumerate_env_lane`: symlink containment canonicalizes `.env/` once, then follows a symlinked entry only if its canonical target `starts_with` the canonical lane, is not the lane itself, and is a regular file. Escaping, dangling, and non-file targets all get the same skip-with-warning.
- `src/age.rs:is_env_secret_stem` implements `^[a-z][a-z0-9-]*$` as a char check instead of compiling a `regex`. Same language, no lazy-static, trivially testable.
- `src/main.rs:build_secrets_env` / `run_secrets_deploy` take the identity resolver as a closure. `secrets_env` / `secrets_deploy` pass `age::resolve_identity(None)`, and tests pass a throwaway x25519 identity. `secrets_env` still prints only in its `Ok` arm, so an `Err` from `build_secrets_env` IS the zero-stdout outcome the tests assert.
- `src/main.rs:render_decrypt` runs `refuse_split_store_sweep` before `resolve_identity`, so a refused sweep never touches an identity and builds no buffer.
- `src/main.rs:default_output_dir` is the `secrets-store` -> `<store>/.env` fallback, pulled out of `resolve_new_output_dir` so it can be tested without discovering a real `manifest.yml`.
- `src/main.rs:lane_warnings_to_stderr(tty, journal)` is the pure TTY-or-`JOURNAL_STREAM` gate. `emit_lane_warning` always `warn!`s and echoes to stderr only when the gate allows it.
- `src/main.rs:reject_legacy_env` runs after `resolve_secrets_root`, so the migration message names the concrete `<root>/.env/` path.

### Deviations
- `config.rs:key_present` returns `std::result::Result<bool, D::Error>`, not the doc's bare `Result<bool, D::Error>`, because `config.rs` imports `eyre::Result`. Same effect, correct seam.
- The doc names a `secrets_env_context` / `secrets_env` change. I also added the `build_secrets_env` and `run_secrets_deploy` seams (identity injection) so the named tests can run end-to-end with a throwaway identity. CLI behavior is unchanged.
- `secrets deploy` runs the legacy-`env:` check, the symlinked-lane-root check, and the collision check BEFORE the `--dry-run` print, so a dry run of a broken layout fails too. The doc says "both lanes" but does not say how dry-run behaves. Failing the plan matches the existing non-bare-name check, which also runs before dry-run.
- `age decrypt <root>/.file` (a sweep of the `.file/` dir itself) is refused too. The walk includes its root entry, which is a `.file` with a sibling `.env`. A single file path is never refused.
- The `$NAME` duplicate check in `enumerate_env_lane` exists as specified but has no test. No two stems that pass `^[a-z][a-z0-9-]*$` can map to the same `$NAME`: `filename_to_var` is injective over that alphabet. So no fixture can reach the branch, and it cannot be bite-checked. The doc already calls it unreachable ("cannot happen under that regex. The lane checks anyway").
- Non-`.age` entries in `.env/` are ignored without a warning. The doc lists warnings only for subdirectories, bad stems, and escaping symlinks. The test asserts that a `notes.txt` produces no warning, which makes the `.age` filter bite independently of the stem check that backstops it.
- Old tests inverted or rewritten, by name:
  - `test_resolve_secrets_dir_defaults_to_sibling_dot_secrets` -> `test_resolve_secrets_root_defaults_to_manifest_dir` (asserts no `.secrets` join)
  - `test_resolve_secrets_dir_uses_secrets_store_override` -> `test_resolve_secrets_root_uses_secrets_store_override`
  - `test_secrets_env_context_rejects_non_bare_name` and `test_secrets_env_context_accepts_bare_names` pinned `secrets.env` list validation. The list is retired, so they are replaced by `test_secrets_env_legacy_list_is_migration_error`, which pins the inverse: any `env:` key now fails.
  - `config.rs` `test_secrets_spec_round_trip` / `test_secrets_unknown_key_fails_deserialization` no longer use `env:`.

### Tradeoffs
- One `check_lanes` for both lanes vs. separate env and deploy checks: one function means the two lanes cannot drift. The cost is that the env lane reads `.env/` twice (once for collisions, once to enumerate). Two `read_dir`s on a dir of ~70 entries is negligible.
- Collision compares raw `.age` entry stems in both lanes (symlinks included, not followed) vs. only the post-containment names. Raw stems are stricter and need no symlink resolution: if a name exists in both dirs in any form, the intent is ambiguous.
- Stray warnings in `secrets deploy` use the same TTY/journal gate as `secrets env` vs. unconditional stderr. Deploy is interactive in practice, so the gate costs nothing and keeps one emitter.

### Open questions
- Two pre-existing tests (`test_linker_leaves_a_correct_dir_symlink_alone`, `test_linker_replaces_a_stale_dir_symlink_in_place`) fail under the Claude Code Bash sandbox. There `TMPDIR=/tmp/claude-1000` is a symlink to `~/.cache/claude-tmp`, and the tests compare a literal path against `read_link`. They fail identically at baseline `e0d20b8` (checked in a detached worktree). `otto ci` is green with `TMPDIR` set to the resolved path. Should those tests canonicalize their tempdir? This is out of Phase 1's scope.

## Phase 2: keep: move the files, drop the list
Commit `16bce34` on branch `secrets-folder-split`, worktree `/home/saidler/repos/scottidler/.wt-keep-secrets-folder-split` (keep, based on main `ebe3b98`).
### Design decisions
- Phases 2 and 3 run on `secrets-folder-split` branches in worktrees, not on the live checkouts (orchestrator's decision) -- keep stays on the old layout until the binary-first rollout, because desk reads the live checkout and an old binary against moved files silently empties every shell's env.
- 4 `syncthing-*.age` -> `.file/`, other 67 -> `.env/` via `git mv`; all 71 are `R100`. The 3 aliases moved with their targets, none dangle (`find -xtype l` = 0).
- `manifest.yml`: whole `secrets:` block removed (it held only `env:`), so no `secrets:` key remains; replaced by a comment block describing the two lanes and why no `secrets.file` is declared. Line 1 header now says `.env/*.age` and `manifest secrets env`.
- `CLAUDE.md`: Quick Reference (emit via `secrets env`, encrypt into `.env/`, list `.env/`), Architecture tree (`.env/` + `.file/`), How It Works (folder is the allowlist; loader line now `secrets env`), Conventions (both lanes, aliases only inside `.env/`).
### Deviations
- Criterion "readlink for each of the 3 aliases shows a bare filename (no `/`)" is not met literally: `claude-cost-anthropic-api-admin-key.age` is `./anthropic-api-admin-key.age`. The other two (`gh-token`, `github-token`) are bare. The link is relative, same-dir, resolves, and is R100, so I left it. Rewriting would break the renames-only criterion.
- The criterion `git show ... -- .secrets .env .file` passes (0 non-R100 of 71). The commit also modifies `CLAUDE.md` and `manifest.yml` (outside those paths, as the phase requires).
### Tradeoffs
- Deleted the `secrets:` key entirely vs. leaving `secrets: {}` -- an empty map is dead config; the comment carries the explanation.
### Open questions
- The dotfiles loader `~/.shell-exports.d/secrets.env` (`dotfiles/HOME/.shell-exports.d/secrets.env`) runs `manifest age decrypt .../keep/.secrets` per keep's old CLAUDE.md, but is not in Phase 3's file list. I could not read it (a hook blocks that filename). After the split `.secrets` does not exist, so it must move to `manifest -C ~/repos/scottidler/keep/manifest.yml secrets env` or shells load nothing. Confirm whether it is in scope for Phase 3 or the rollout.
- Alias `claude-cost-anthropic-api-admin-key.age -> ./anthropic-api-admin-key.age`: fix to a bare name in a separate commit, or relax the criterion?

## Phase 2 orchestrator addendum: criterion amendment and loader check

### Design decisions
- `dotfiles/HOME/.shell-exports.d/secrets.env` needs no change: it is a 2-line, comment-only stub (`rg -vc '^\s*(#|$)'` = 0 non-comment lines, 0 `manifest`/`decrypt` matches). The live loader is `.zshenv:13`, already `manifest -C .../keep/manifest.yml secrets env`.

### Deviations
- Amended the Phase 2 alias criterion (doc defect, not a code failure). It said each alias's `readlink` is a bare filename with no `/`. On main, `readlink keep/.secrets/claude-cost-anthropic-api-admin-key.age` = `./anthropic-api-admin-key.age` (since `c37aeed`, 2026-03-28). Making it bare means rewriting the link, which contradicts the same phase's renames-only criterion (all R100). The criterion's intent is a same-dir relative target, and `find .env -xtype l | wc -l` = 0 confirms every alias resolves. New wording: bare filename or `./<name>`, no `..`, not absolute.

### Tradeoffs
- None.

### Open questions
- None.

## Phase 3: callers that hardcode a ciphertext path
### Design decisions
- spotify-playlist SKILL.md and spotify.py point at `keep/.env`: the refresh token is an env secret hydrated as `$SPOTIFY_REFRESH_TOKEN` (claude/HOME/.claude/skills/spotify-playlist).
- rules/secrets.md intro names both lanes (`.env/` env, `.file/` deployed to disk). The "all" line is `manifest -C ~/repos/scottidler/keep/manifest.yml secrets env`, with a note that `age decrypt <dir>` on keep is refused (claude/HOME/repos/.claude/rules/secrets.md).
- entropy-check allow-set is `d in (".env", ".file")`, comment updated (dotfiles/HOME/bin/entropy-check).
### Deviations
- secrets.md: the doc lists lines 9,21,22,30,48. Line 9's sentence grew a `.file/` clause (one extra wrapped line), and lines 21-22 collapsed into the one "all" bullet plus a refusal note. Same effect.
- Success-criterion greps run with bracketed regex `keep/[.]secrets` and `[.]secrets` (a hook mangles `-F` path tokens). Both = 0 (baseline 12).
### Tradeoffs
- Left the `$SP/.secrets` scratch-dir mentions in intent-guard-test.sh:228 and intent-guard.sh:1862 alone: unrelated to keep.
### Open questions
- None. Both commits are local on `secrets-folder-split` in the claude and dotfiles worktrees, not pushed.

## Audit round 1 fold-in: manifest
### Design decisions
- Split-store refusal runs on canonical paths (src/age.rs:refuse_split_store_sweep, enclosing_file_lane). It canonicalizes `<dir>` and every `.age` from `find_age_files`, then refuses if either has an ancestor named `.file` whose parent holds `.env`. The walk for a split store nested below `<dir>` stays. A dangling `.age` link is skipped by the check (trace): it decrypts to nothing and render already reports it.
- A relative `age decrypt` path is resolved against the cwd before the check and the sweep (src/main.rs:render_decrypt -> render_decrypt_from). The check and the sweep see the same directory, and tests pass a tempdir cwd instead of changing process cwd.
- `resolve_secrets_root` maps an empty manifest parent (`-C manifest.yml`) to `.` (src/main.rs:resolve_secrets_root).
- `find_strays` logs a `warn!` when `read_dir` on the root fails, instead of returning no strays (src/age.rs:find_strays).
### Deviations
- The refusal message says "it reaches the file lane '<lane>'" and names one lane path, not the `.file`/`.env` pair. The canonical cases (a symlink out of `.env/`, a cwd inside the lane) have no nested pair to name. It still points at `manifest secrets env`.
- In the live binary, the cwd join alone closes `cd .file && age decrypt .` (`/abs/.file/.` names `.file` to the old walk). The canonical-dir check guards the variant with no `.file` component, `cd .file/certs && age decrypt .`, so the cwd test covers both spellings.
### Tradeoffs
- Kept the canonical-dir check even though the per-file check refuses every case that would print a secret. Without it, a sweep of an empty or link-only spelling of a file lane succeeds with empty output; with it, the answer is always a refusal. test_refuse_split_store_sweep_canonical_dir_inside_file_lane bites it.
- The stray test asserts the resolved root (`.` for both spellings) plus a `find_strays` call on the tempdir joined with it, over changing process cwd. The `.` assertion is the part that bites. Live probe on the debug binary: 1 stray warning under each of `-C manifest.yml` and `-C ./manifest.yml`.
### Open questions
- None.

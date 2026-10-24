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

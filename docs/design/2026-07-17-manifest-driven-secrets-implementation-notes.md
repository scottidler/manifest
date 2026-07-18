# Implementation Notes: manifest.yml-driven secrets

Running, append-only record of how the implementation diverges from or interprets
the design doc (`2026-07-17-manifest-driven-secrets.md`). Append per phase; never
edit history.

## Phase 0: Prove the fail-soft + format contracts

### Design decisions
- Verified the load-bearing shell/eval contracts empirically in `zsh` (the actual
  `.zshenv` shell), not just by reasoning:
  - `eval "$(false)"` -> shell alive, exit 0.
  - missing binary (`eval "$(nonexistent 2>/dev/null)"`) -> shell alive, exit 0.
  - zero-stdout non-zero exit (`eval "$(sh -c 'exit 3')"`) -> shell alive, exit 0.
  - **Hazard proven:** `eval "$(printf 'export LEAK=ok\n'; exit 9)"` -> `LEAK=ok`
    survives. Stream-then-fail DOES mutate the shell; this is exactly why the emit
    path must buffer and emit zero stdout on failure. Confirms the Phase 2 contract.
  - hand-written `export` block eval's clean.

### Deviations
- None.

### Tradeoffs
- The `-f env` vs `-f export` format contract and the symlink-alias decrypt were
  NOT re-derived with a throwaway identity: `manifest age --keygen` targets the
  real default identity (`~/.config/manifest/identity.txt`) and correctly refuses
  to overwrite it, and `age-keygen` is not installed. Fabricating a throwaway
  identity was not worth the risk near the real key. Instead the format contract
  stands on direct code read + existing tests: `render_env` -> `env_escape`
  (systemd double-quoted, tests at `age.rs:1404` cover space/`"`/`'`/`\`),
  `render_exports` -> `shell_escape` (ANSI-C `$'...'`). The symlink contract stands
  on the LIVE production alias (`gh-token.age` -> `github-pat-work.age` decrypts in
  every shell today) plus `find_age_files`/WalkDir resolving a symlink to its
  `.age` target (`age.rs:72`). These are re-asserted as executable tests in
  Phase 2 (systemd-parse of `-f env` special chars) and Phase 0's success criteria
  are otherwise met.

### Open questions
- None.

## Phase 1: Schema

### Design decisions
- `SecretsSpec { env: Vec<String>, file: HashMap<String, String> }` added to
  `src/config.rs`, `#[serde(deny_unknown_fields)]`, plain lowercase field names
  matching the yaml keys (no rename) -- exactly per the design doc. Wired into
  `ManifestSpec` as `#[serde(default)] pub secrets: SecretsSpec`, next to
  `script:` (`src/config.rs:82`), so a manifest with no `secrets:` block still
  deserializes.
- Tilde-expansion of `secrets.file` destination values lives in
  `load_manifest_spec` (`src/config.rs:32`), the exact same post-deserialization
  hook `secrets_store` already uses -- `PathBuf`/`from_reader` do not expand `~`,
  so this is a deliberate second pass over `parsed.secrets.file.values_mut()`.
  Only the destination path is touched; the map key (secret name) is never
  passed through `expand_tilde`.
- `const SECRET_FILE_MODE: u32 = 0o600;` added to `src/age.rs` (not a new
  `secrets` module) per the team lead's routing instruction -- Phase 3's
  `secrets deploy` lane lives in `age.rs` alongside the other atomic-write/chmod
  primitives it will reuse (`encrypt_named`'s atomic-write skeleton,
  `generate_identity`'s chmod idiom). It is unused at Phase 1, so it is gated
  `#[allow(dead_code)]` with a doc comment stating it is consumed in Phase 3.
  This is a deliberate, temporary exception to the repo's "never
  `#[allow(dead_code)]`" rule for an explicitly transitional, phase-gated const;
  it must be removed (the attribute, not the const) the moment Phase 3 wires a
  call site.
- Tests live inline in `#[cfg(test)] mod tests` at the bottom of `config.rs`,
  matching this repo's existing convention (documented in the repo's
  `CLAUDE.md`), not the separate-test-file convention used elsewhere.

### Deviations
- None. Schema, field names, `deny_unknown_fields`, no `FileSecret` struct, and
  the tilde-expansion hook all match the design doc's Architecture section
  exactly.

### Tradeoffs
- Considered adding a new `src/secrets.rs` module for `SECRET_FILE_MODE` instead
  of `age.rs`. Chose `age.rs` because Phase 3's deploy lane explicitly reuses
  `age.rs`'s atomic-write skeleton and chmod idiom (design doc Architecture
  section); putting the mode const anywhere else would just require moving it
  again in Phase 3 for no benefit now.
- `expand_tilde` takes a `PathBuf`, so expanding a `String` map value requires a
  `PathBuf::from` / `to_string_lossy` round trip per entry. Considered changing
  `secrets.file`'s value type to `PathBuf` instead of `String` to avoid this,
  but the design doc is explicit that `file` is `HashMap<String, String>` (the
  same type as `LinkSpec.items`) -- kept `String` and paid the small round-trip
  cost in the load hook rather than diverge from the documented type.

### Open questions
- None.

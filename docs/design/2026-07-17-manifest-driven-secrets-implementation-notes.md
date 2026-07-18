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

## Phase 2: secrets env emit path

### Design decisions
- New buffered emit path `render_secrets_env(names, secrets_dir, identity, format)`
  in `src/age.rs` (beside `render_env`/`render_exports`). It iterates ONLY `names`
  (the `secrets.env` allowlist), resolves each to `<secrets_dir>/<name>.age`,
  decrypts in-process, and formats with `filename_to_var` + escaper-by-format:
  `DecryptFormat::Export -> shell_escape` (ANSI-C `$'...'`), `DecryptFormat::Env ->
  env_escape` (systemd double-quoted). It does NOT call `render_exports`/`render_env`
  (those walk a directory and carry the poison placeholder). Returns the COMPLETE
  buffer as a `String`; per-secret decrypt failures are skipped with a `warn!` (log
  only, never stdout) and NO placeholder.
- CLI: `Commands::Secrets { action: SecretsAction }` + `SecretsAction::Env { format }`
  reusing `DecryptFormat` (default `export`) in `src/cli.rs`. Room left for `deploy`
  (Phase 3) as a second `SecretsAction` variant, with a `// deploy in Phase 3`
  marker; not implemented now. Dispatch wired in `main.rs` (`handle_secrets_command`).
- Buffer-then-emit-once is enforced structurally in `main.rs::secrets_env`: the whole
  output `String` is built first, then `print!` runs in exactly ONE place, the `Ok`
  arm. A command-level error returns before that `print!`, so stdout is guaranteed
  empty on failure (the fail-soft `.zshenv` contract). The command-level `Err` also
  writes the interactive banner (`secrets_env_banner`) to STDERR gated on
  `std::io::stderr().is_terminal()`.
- Two failure classes kept distinct: per-secret decrypt error -> skip + `warn!` in
  `render_secrets_env`; command-level error (unparseable manifest, no parent dir,
  unresolvable identity) -> zero stdout, non-zero exit (the returned `Err`), plus the
  TTY-gated stderr banner. `secrets_env_context` performs config-load + secrets-dir
  resolution BEFORE identity resolution, so a bad manifest aborts ahead of the emit
  step and never touches the real identity.
- Secrets-dir resolution (`resolve_secrets_dir`): `secrets-store` override if set,
  else `<dir-of-manifest.yml>/.secrets` (the default sibling store), mirroring how
  `link:` resolves sources relative to the repo root. `-C/--config` (`cli.config`) is
  threaded into the handler so `.zshenv`'s `manifest -C .../keep/manifest.yml secrets
  env` resolves against the keep manifest.
- Tests inline in `#[cfg(test)] mod tests` per this repo's convention; fixtures
  encrypted with a throwaway `age::x25519::Identity::generate()` inside each test,
  never the real `~/.config/manifest/identity.txt`.

### Deviations
- `render_secrets_env` lives in `age.rs` and takes `&DecryptFormat`, so `age.rs` now
  imports `crate::cli::DecryptFormat`. This points the "core" module at a `cli` type;
  chosen because `DecryptFormat` is the existing typed format pivot (already the
  escaper selector in `main.rs`) and reproducing it in `age.rs` would duplicate the
  vocabulary. No import cycle (`cli.rs` does not import `age`). Same effect as the
  doc's "select escaper by `DecryptFormat`", implemented at the render seam.
- The command-level stderr banner is `manifest: secrets env failed, N secrets not
  loaded` ONLY when the manifest parsed (N = `secrets.env.len()`). When the config
  itself never parsed, the count is genuinely unknown, so the banner omits the number
  (`manifest: secrets env failed, secrets not loaded`) rather than fabricate a count.
  The doc's banner text names `N`; this is the honest degradation for the
  no-count-available case.
- The `-f env` systemd-loadability test does NOT invoke a real systemd parse. A
  `systemd-run --user --property=EnvironmentFile=...` was verified unavailable in this
  sandbox/CI (no user session bus: "Failed to connect to user scope bus ... Operation
  not permitted"), the explicitly-permitted fallback. Instead the test asserts the
  emitted line equals `env_escape`'s output, contains NO ANSI-C `$'...'` (the
  discriminator proving `-f env` routes through `env_escape`, not `shell_escape`), and
  that the double-quoted value uses only systemd's documented escapes (`\"`, `\\`,
  `\n`). The `-f export` test DOES run a real `bash -c 'eval ...'` and round-trips the
  exact value.

### Tradeoffs
- Passing `&DecryptFormat` into `render_secrets_env` vs. having `main.rs` pick the
  escaper and pass a closure/bool: chose the typed enum so the buffering + skip +
  no-placeholder logic (the load-bearing core) lives and is tested in one place in
  `age.rs`, at the cost of the `age -> cli` type import above.
- Command-level "zero stdout" is asserted structurally (a command-level failure
  returns `Err` from `secrets_env`/`secrets_env_context`, and `print!` only exists in
  the `Ok` arm) rather than by capturing process stdout in-process, which is not
  cleanly doable in a unit test. The structural guarantee is stronger than a captured
  snapshot.
- The interactive stderr banner's TTY gate (`is_terminal()`) is not unit-tested for
  emission (a TTY cannot be faked portably); only the pure `secrets_env_banner`
  message builder is tested, for both the known-count and unknown-count cases.

### Open questions
- None.

## Phase 3: secrets deploy file lane (native Rust)

### Design decisions
- Native deploy lives entirely in `src/age.rs`, NOT the `ManifestType` render-to-Bash
  path. Decrypted bytes reach ONLY the destination file - never stdout, a generated
  Bash string, or a process arg. Five functions:
  - `create_secret_temp(dir, base)` (`src/age.rs`) - opens the temp at
    `SECRET_FILE_MODE` (`0600`) FROM CREATION via `OpenOptionsExt::mode` +
    `create_new(true)`, in the destination's OWN directory. Not `encrypt_named`'s
    `0644`-then-nothing nor `generate_identity`'s chmod-after; this closes the
    world-readable plaintext window the design flags.
  - `ensure_secret_parent(parent)` - `symlink_metadata` on the LITERAL parent path
    to refuse a symlinked parent; if the parent is missing, `DirBuilderExt::mode`
    (`0700`) + recursive `create`, then asserts the resulting mode is `0700`.
  - `write_secret_file(dest, bytes)` - ensure parent -> temp at `0600` -> write ->
    `sync_all` -> `rename` over dest -> final `set_permissions(0600)` -> `fsync`
    the parent dir. Temp removed on any failure; dest untouched.
  - `deploy_secret_file(name, dest, secrets_dir, identity)` - `decrypt_file`
    `<secrets_dir>/<name>.age` in-process, then `write_secret_file`.
  - `deploy_secret_files(entries, secrets_dir, identity) -> DeployReport` - batch
    loop, per-file atomic, report-and-continue; returns a `DeployReport { deployed,
    failed }` as DATA (no printing, no exit) so the shell owns stdout/stderr/exit.
- `SECRET_FILE_MODE` had its `#[allow(dead_code)]` REMOVED (Phase 1 marked it
  explicitly temporary until a Phase 3 call site existed). Added a sibling
  `const SECRET_DIR_MODE: u32 = 0o700;` for the parent-dir mode rather than a bare
  `0o700` literal (per rust.md "no magic numbers").
- `secrets_deploy(config, dry_run)` (`src/main.rs`) dispatched from
  `handle_secrets_command` next to the Phase 2 `Env` handler. It loads the manifest,
  resolves the secrets dir via the shared `resolve_secrets_dir`, sorts the
  `secrets.file` entries by name (a `HashMap` iterates non-deterministically), and:
  - `--dry-run`: prints `name -> path` per entry and returns BEFORE resolving an
    identity or reading any ciphertext (decrypts nothing, writes nothing).
  - otherwise: resolves the identity, calls `deploy_secret_files`, prints
    `deployed: name -> path` for each success and `manifest: failed to deploy secret
    '<name>': <err>` to stderr for each failure, and returns `Err` (non-zero exit)
    if ANY entry failed.
- CLI: `SecretsAction::Deploy { dry_run: bool }` with `--dry-run` (`src/cli.rs`),
  replacing the Phase 2 `// deploy in Phase 3` placeholder comment.

### Deviations
- The design's Phase 3 bullet lists the deploy step as one inline sequence; it is
  implemented as four seams (`create_secret_temp`, `ensure_secret_parent`,
  `write_secret_file`, `deploy_secret_file`) plus the batch `deploy_secret_files`.
  Same effect, correct seams: the split makes the two load-bearing invariants
  (temp `0600`-at-creation; parent `0700`/symlink-refusal) unit-testable in
  isolation without a real identity or a decrypt.
- The batch loop returns a `DeployReport` (data) and `main.rs` maps a non-empty
  `failed` list to a non-zero exit, rather than the loop itself performing the
  report-and-exit. This keeps age.rs side-effect-free (rust.md "return structured
  data, not side effects") and lets the mid-batch test assert continuation with a
  throwaway identity. Same "report-and-continue, non-zero exit if any failed"
  behavior the design specifies.

### Tradeoffs
- The final `set_permissions(0600)` after rename is redundant with the temp's
  creation mode (the temp is already `0600` and `rename` preserves it), but kept
  as the design's explicit "chmod final 0600" step - it also normalizes a
  pre-existing destination whose mode differed. The security guarantee rests on the
  CREATION mode, not this chmod.
- `ensure_secret_parent` asserts `0700` only for a parent it CREATED; a pre-existing
  parent's mode is left alone (a legitimate `~/.config/syncthing` may be `0755`).
  Refusing a non-`0700` existing parent would break real destinations for no
  security gain, since the file itself is `0600` regardless.
- The symlink refusal checks only the IMMEDIATE literal parent (per the design); a
  symlinked deeper ancestor and the residual check-to-write TOCTOU are out of scope
  for this threat model (a local attacker who can plant a symlink under `~/.ssh`
  already has profile write access), as the design's Security section states.
- The "plaintext never in stdout" criterion is proven via
  `test_deploy_report_carries_no_plaintext` (the report the caller prints carries
  only names/paths/errors, never a value) rather than capturing process stdout,
  which is not cleanly doable in a unit test. The guarantee is structural: the only
  things `secrets_deploy` prints are report names + declared dest paths.

### Open questions
- None.

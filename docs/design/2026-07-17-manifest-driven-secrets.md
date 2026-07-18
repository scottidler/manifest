# Design Document: manifest.yml-driven secrets (env | file targets)

**Author:** Scott A. Idler
**Date:** 2026-07-17
**Status:** In Review
**Review Passes Completed:** 5/5 (+ 3 cross-model review panels: schema ranking, full-doc, revised full-doc)

## Summary

Make `manifest.yml` the single declarative driver for age-encrypted secrets.
A secret is declared in one of two blocks: `env` (a list of names, each emitted
as a shell `export`) or `file` (a name -> path map, each decrypted to that path
at mode `0600`, in-process, never through the environment). Every consumer that
reads the secret store today (the shell plus the borg and cortex daemons)
migrates to the new declarative command, and the old whole-directory sweep is
retired. Result: a `file` secret physically cannot reach any environment, shell
or daemon.

## Problem Statement

### Background

- `manifest` reads `manifest.yml` and renders Bash for first-class patterns:
  `link:`, `github:`, `git-crypt:`, package lists. `script:` is the imperative
  **catchall** for anything without a structured shape.
- Secrets are the exception: NOT manifest.yml-driven. Every consumer runs a
  blanket `manifest age decrypt <dir>` sweep that dumps every `.age` in
  `.secrets/` into an environment, ignoring any per-secret intent.
- "Decrypt a secret to a path and chmod it" currently lives in the `script:`
  catchall (the `git-crypt` `post_unlock: chmod 600 ~/.ssh/id_rsa` pattern).
  A recurring, structurable pattern belongs in a first-class block.

### The three live consumers of the dir-sweep (verified)

- `dotfiles/HOME/.zshenv:13` -- the interactive shell.
- borg daemon -- `dotfiles/HOME/.config/sb/borg.yml:169`
  (`manifest age decrypt keep/.secrets -f env`).
- cortex daemon -- `dotfiles/HOME/.config/sb/cortex.yml:140` (same).

The shell reads its command directly from `.zshenv`. The two daemon commands
live in **dotfiles config**, not `sb` source -- so `sb`'s Rust is untouched --
BUT that config is the *source* that `sb borg daemon --install` /
`sb cortex daemon --install` splices into the generated systemd units. The
**executable state** is the live unit files, which already carry the sweep:
`~/.config/systemd/user/borg.service:10` and `cortex.service:7` both run
`ExecStartPre=... manifest age decrypt <keep/.secrets> -f env > /run/user/1000/<d>.env`
then `EnvironmentFile=-/run/user/1000/<d>.env`. Editing the yml does NOT rewrite
those units: the migration must re-run the daemon `--install` (or hand-edit the
units), `daemon-reload`, restart, and probe. See Phase 5.

### Problem

- **Secrets are not declared.** The env set is whatever `.age` files sit in
  `.secrets/`. No manifest record of what exists or where it goes.
- **The sweep is not fail-closed.** On a decrypt failure the current path emits
  a poison placeholder instead of skipping, in BOTH render paths:
  `render_exports` emits `export NAME='manifest age command failed'`
  (`src/age.rs:1131`) and `render_env` emits `NAME="manifest age command failed"`
  (`src/age.rs:1170`). Both go.
- **File-shaped secrets are wrongly forced into env, in three places.** TLS
  material (`syncthing-cert-desk.age`, `syncthing-key-desk.age`,
  `syncthing-cert-ltl-7007.age`, `syncthing-key-ltl-7007.age`) is swept into the
  shell env AND both daemons' envs. TLS keys have no business in the environment
  of every shell, agent, subprocess, and daemon.
- **No lane for a key file.** The motivating case: a passphrase-less SSH signing
  key (`~/.ssh/identities/work/signing`, mode `0600`) must live encrypted at
  rest, deploy to a path, and never enter any env. keep has no shape for this:
  `.secrets/` = env, `HOME/`/`copy/` = plaintext. Neither is
  encrypted-file-to-disk.

### Goals

- `manifest.yml` is the single source of truth for every secret and its sink.
- Two sinks: `env` (export/env lines) and `file` (decrypt to a path at `0600`).
- **Fail-closed, system-wide.** Only an `env`-block secret is ever emitted into
  an environment, by ANY consumer. A `file` secret, or anything undeclared,
  reaches no env -- shell or daemon. Enforced by migrating all three consumers
  to the new command and retiring the dir-sweep.
- The `.zshenv` swap is fail-soft: a manifest error leaves the shell alive.
- Migrate the existing 46 `.age` files (+ 3 symlink aliases = 49 names) into the
  new declaration.

### Non-Goals

- Not touching the `age encrypt` subcommand or the identity mechanism
  (`~/.config/manifest/identity.txt`). The single-file `age decrypt <file>`
  primitive stays (used ad hoc) and keeps its `render_exports`/`render_env`
  formatters (de-poisoned); only the whole-directory sweep-into-env *usage* is
  retired, not the format functions.
- **Per-file mode override is deferred, not excluded.** Every file secret
  deploys at `0600`. Revisit condition: a real secret needs a different mode.
  None of the current set does. An override later must not reintroduce the
  mixed-value-type parse trap (see Alternatives) -- likely a separate
  `file-modes:` map, designed then.
- Not adding a secrets *manager* (rotation, expiry, remote fetch). keep is a
  local age store, not Vault.
- Not converting `link:`/`git-crypt:` deployments. Parked: only secrets move to
  the new lanes now.
- Not cleaning up the two plaintext `.secrets/*.txt` datadog keys or the
  `keep/CLAUDE.md` doc drift. Parked as operator hygiene (see Addendum).

## Proposed Solution

### Overview

- New `secrets:` section in `manifest.yml`, two homogeneous blocks:
  - `env:` a list of secret names. Each name -> its `.age` file -> `$NAME` via
    the existing `filename_to_var` (lowercase-kebab -> UPPER_SNAKE).
  - `file:` a map of secret name -> destination path. Each name -> its `.age`
    file, decrypted to that path at mode `0600`.
- Two native subcommands:
  - `manifest secrets env [-f export|env]` -> emits `export NAME='value'`
    (`export`, default, for the shell) or `NAME=value` (`env`, for a systemd
    `EnvironmentFile`) for every `secrets.env` entry. Reuses the existing
    `DecryptFormat` enum (`src/cli.rs:279`).
  - `manifest secrets deploy [--dry-run]` -> for every `secrets.file` entry:
    decrypt -> atomic write to the path at `0600`. In-process Rust. Never Bash.
- **All three consumers migrate to `secrets env`; the dir-sweep is retired.**

### Why this shape

- **`env` is the package-block idiom; `file` is the `link:` idiom.** `env` as
  `Vec<String>` matches `pkg:`/`apt:`/`cargo:` (`config.rs:110`). `file` as
  `HashMap<name, path>` is the SAME TYPE as `LinkSpec.items`
  (`HashMap<String,String>`, `config.rs:107`): a repo-sourced thing mapped to a
  destination. Both blocks reuse an existing manifest shape; neither is a new
  special case.
- **Both value types are homogeneous, so there is no parse trap.** Neither block
  mixes a scalar and a map, which is the `link:`/`dirs` type-discrimination
  complexity manifest deferred (`config.rs:99-103`). This is why mode is NOT a
  per-entry field: `{path, mode}` would make `file` values maps and reintroduce
  that trap.
- **Fail-closed is structural:** the env emitter reads only `secrets.env`; a
  `file` secret has no representation in the env lane. No `target:` string to
  typo into the wrong lane. With all consumers on this command and the sweep
  gone, the guarantee holds everywhere.

### Architecture

- `ManifestSpec` gains one field: `secrets: SecretsSpec` (`src/config.rs:44`).
- `SecretsSpec { env: Vec<String>, file: HashMap<String, String> }` with
  `#[serde(deny_unknown_fields)]`. Keys plain lowercase, no serde rename; field
  name equals yaml key. No `FileSecret` struct -- a file value is just the path.
- File mode is a single module const, `const SECRET_FILE_MODE: u32 = 0o600;`,
  not config (override deferred per Non-Goals).
- `secrets env` reuses `filename_to_var` and the existing `DecryptFormat` enum
  for `-f export|env`, and picks the escaper by format exactly as the current
  render path does: `Export -> shell_escape` (ANSI-C `$'...'` for the shell,
  `age.rs:144`), `Env -> env_escape` (double-quoted, systemd-`EnvironmentFile`-
  parseable, `age.rs:106`). `shell_escape` output is NOT systemd-loadable, so
  `-f env` MUST route through `env_escape`, never `shell_escape`. The new emit
  path is its OWN buffered function -- it does NOT call `render_exports`/
  `render_env` (those walk a directory and carry the poison placeholder); it
  **buffers all output and emits once** (or emits nothing on a command-level
  error) -- see fail-soft below.
- `secrets deploy` does NOT use the `ManifestType` render-to-Bash path. Routing
  decrypted bytes through generated Bash would expose them in the script text,
  stdout, and process args. It is native Rust:
  - **create the temp file at `0600` from creation** via `OpenOptionsExt::mode`,
    NOT write-then-chmod (the reused `encrypt_named` opens with no mode ->
    `0644`, and `generate_identity` chmods after write -- both leave a
    world-readable plaintext window; that is fixed here).
  - write -> `sync_all` -> rename over destination -> chmod final `0600` ->
    fsync parent dir. Atomic-write skeleton from `encrypt_named` (`src/age.rs:775`),
    chmod idiom from `generate_identity` (`src/age.rs:950`).
  - **create the parent at `0700`** if missing (a fresh machine has no
    `~/.ssh/identities/work/`) via `DirBuilderExt::mode(0o700)` + `create_dir_all`
    (plain `create_dir_all` applies `0777 & umask`, NOT `0700`), then assert the
    resulting mode.
  - **refuse a symlinked parent:** `canonicalize()` resolves symlinks, so calling
    `.is_symlink()` on its result is ALWAYS false (mechanically useless). Reject
    instead via `symlink_metadata` on the literal parent path (or compare the
    canonical path against the literal absolute path), so a planted symlink cannot
    redirect the write. A residual check-to-write TOCTOU remains but is out of
    scope for this threat model (a local attacker who can plant a symlink in
    `~/.ssh/` already has profile write access): the guarantee is "does not follow
    a symlink present at check time", not "cannot ever be redirected".
  - **batch semantics:** per-file atomic; on a failed entry, report it and
    continue with the rest; exit non-zero if any failed.

### Data Model

```yaml
# keep/manifest.yml  (at repo root; .secrets/ is its sibling)
secrets:
  env:                  # name -> ./.secrets/<name>.age -> $NAME
    - github-pat-work
    - github-pat-home
    - gh-token          # symlink alias -> github-pat-work.age; own env var $GH_TOKEN
    - github-token      # symlink alias -> github-pat-work.age; own env var $GITHUB_TOKEN
    # ... the rest of the env-target names

  file:                 # name -> destination path; ./.secrets/<name>.age deployed there at 0600
    work-signing-key: ~/.ssh/identities/work/signing
    syncthing-cert-desk: ~/.config/syncthing/cert.pem
    syncthing-key-desk: ~/.config/syncthing/key.pem
```

- **Storage is unchanged.** The encrypted `.age` files stay in `keep/.secrets/`
  exactly as today; nothing relocates. This plan changes how they are *declared*
  (a `secrets:` block instead of a directory sweep) and, for the `file` sink,
  adds a decrypted-copy *destination*; the ciphertext-at-rest location does not
  move. For a `file` secret, only the decrypted output lands at a new path
  (`~/.ssh/...`); the `.age` itself never leaves `keep/.secrets/`.
- The secret name is the single knob. A bare name resolves to its ciphertext at
  `<dir-of-manifest.yml>/.secrets/<name>.age`. No store path needs declaring:
  the loaded manifest's own path gives the repo root
  (`load_from_standard_locations` returns it; `discover_repo_root` walks it),
  exactly as `link:` resolves sources relative to the repo root. `secrets-store`
  stays the `age encrypt` output dir (`config.rs:82-92`), reused only as an
  OPTIONAL override if a store ever lives off the default `./.secrets`.
- Symlink aliases (`gh-token.age -> github-pat-work.age`) are listed by their
  own name in `secrets.env`. Each decrypts independently to its own env var
  today; listing them by name preserves that with no alias-specific logic.

### API Design

```
manifest secrets env               # export NAME='value' lines (shell); default format
manifest secrets env -f env        # NAME=value lines (systemd EnvironmentFile; borg/cortex)
manifest secrets deploy             # decrypt each secrets.file entry -> atomic write -> chmod 0600
manifest secrets deploy --dry-run   # print planned name -> path per entry, decrypt nothing
```

- `secrets env` is the ONLY command that emits an environment, and it is an
  allowlist over `secrets.env`. Nothing else emits env after the sweep is retired.
- On a per-secret decrypt failure:
  - `secrets env`: skip that entry, emit nothing for it, `warn!` to the log file
    (never stdout). No poison placeholder.
  - `secrets deploy`: report the failed entry, continue with the rest, exit
    non-zero at the end. A key half-written is worse than absent (atomic write
    guarantees no partial file for the failed one).

### Fail-soft (the `.zshenv` contract)

- `secrets env` **buffers all export lines and prints once**, or prints nothing
  on a command-level error. A partial env is therefore impossible even on a
  mid-run panic (a panic unwinds and drops the buffer; an OOM `SIGKILL` writes
  nothing). This is a stated requirement, not an accident of the current code.
- Two failure classes, kept distinct: a per-secret decrypt error -> skip that one
  (allowed); a command-level error (bad config, missing identity, or the whole
  command failing) -> zero **stdout**, non-zero exit. Zero stdout is the safety
  property; "partial stdout survives" is the hazard (a `NAME=val` line already
  emitted mutates the shell even if the process later exits non-zero), so the
  buffer-then-emit-once discipline is load-bearing, not cosmetic.
- `eval "$(manifest ... secrets env)"` then can never break the shell: on any
  error the command substitution is empty and `eval` is a no-op.
- **Visible-on-failure, not silent (machine-lag guard).** stdout stays empty on a
  command-level failure, but the command ALSO writes a one-line banner to
  **stderr** -- `manifest: secrets env failed, N secrets not loaded` -- so a
  lagging machine (new shared `.zshenv`, old `manifest` with no `secrets env`
  subcommand) shows the operator the drop instead of silently losing every
  secret. The banner is gated on an interactive shell (`.zshenv` runs for
  non-interactive shells too; unguarded stderr there breaks `scp`/`rsync`/
  `git`-over-ssh). Shell survives (fail-soft) AND the failure is visible
  (fail-loud) -- both, per the owner's "degrade visibly" standard. Belt-and-
  suspenders with the ship-order version gate below, which prevents the window in
  the first place.
- A hang has no buffer-based defense (buffer-once cannot help a command that
  never returns); the mitigation is that `secrets env` does no network I/O (local
  age decrypt only) and the same work runs in ~24ms today.

### Implementation Plan

Ship order (forced by blast radius): manifest code -> installed AND version-proven
on every machine -> keep declares -> migrate the consumers (shell + both live
daemon units) -> retire the sweep. manifest must exist everywhere before any
consumer calls `secrets env`. The install step is a concrete gated action, not
prose between phases: on each of desk/lappy/mini, prove
`manifest secrets env --help` exits 0 (subcommand present) BEFORE the shared
`.zshenv` flip lands. The stderr banner (fail-soft section) is the safety net if
the gate is ever mis-sequenced.

#### Phase 0: Prove the fail-soft + format contracts
**Model:** sonnet
- Zero code. Confirm: `eval "$(false)"` is a no-op that leaves the shell alive;
  a missing binary and a zero-stdout non-zero exit both leave a usable shell;
  a hand-written `export` block eval's clean; a symlinked `.age` decrypts; the
  current `-f env` vs `-f export` outputs are what borg/cortex vs the shell
  expect. **Also confirm the hazard directly:** a producer that emits a partial
  `NAME=val` line then exits non-zero DOES mutate the shell -- proving why the
  emit path must buffer and emit zero stdout on failure, not stream.
- **Success criteria:** `eval "$(false)"` subshell exits 0 and survives; a
  partial-stdout-then-fail producer is shown to leak into the shell (the hazard
  buffer-then-emit closes); a symlinked `.age` yields its line; the `-f env`
  output is bare `NAME=value` / systemd-double-quoted for special chars
  (systemd-loadable via `env_escape`) and `-f export` is `export NAME=...` /
  ANSI-C `$'...'` for special chars (`shell_escape`).

#### Phase 1: Schema
**Model:** sonnet
- Add `SecretsSpec` to `src/config.rs`; wire `secrets` into `ManifestSpec`.
  `deny_unknown_fields`. Tilde-expand each `secrets.file` value in
  `load_manifest_spec` (same hook as `secrets_store`).
- **Success criteria:** a `secrets.env` list + a `secrets.file` map parse; a
  `secrets.file` value that is a map (not a string) fails deserialization
  (homogeneity held); an unknown key under `secrets:` fails; a manifest with NO
  `secrets:` block still deserializes.

#### Phase 2: `secrets env` emit path (allowlist, both formats, buffered)
**Model:** opus
- New emit path iterating ONLY `secrets.env`, reusing `filename_to_var` +
  `DecryptFormat`, and selecting the escaper by format: `Export -> shell_escape`,
  `Env -> env_escape` (systemd-parseable; NEVER `shell_escape` for `-f env`).
  Buffer fully, emit once; on a per-secret decrypt error skip + `warn!`; on a
  command-level error emit zero stdout + write the interactive stderr banner,
  non-zero exit. No poison placeholder anywhere. This path does NOT call
  `render_exports`/`render_env`.
- **Success criteria:** a `secrets.file` secret never appears in output; a forced
  per-secret decrypt failure emits nothing for that var (no placeholder string
  survives in the binary); a command-level failure emits zero stdout AND the
  interactive-only stderr banner; `-f env` output for a value containing a space,
  `"`, `$`, and a literal `'` is accepted by a real systemd `EnvironmentFile`
  parse (not just shell `eval`); the emitted name/value SET equals the legacy
  sweep's for the env-target set (compare sets, not bytes -- `find_age_files` does
  not sort, `age.rs:80`).

#### Phase 3: `secrets deploy` file lane (native Rust)
**Model:** opus
- decrypt -> temp file created at `0600` (OpenOptionsExt::mode, before writing)
  in the target dir -> `sync_all` -> rename -> chmod `0600` -> fsync parent.
  `create_dir_all` parent at `0700` if missing. Refuse a symlinked parent
  (canonicalize + assert). Per-file atomic; report-and-continue on failure,
  non-zero exit if any failed. `--dry-run` prints name -> path, decrypts nothing.
- **Success criteria:** deployed file is `0600` (metadata assert); the temp file
  is never broader than `0600` at any instant (asserted via the creation mode,
  not a post-hoc chmod); a mid-batch decrypt failure leaves zero partial file for
  that entry, deploys the others, and exits non-zero; a symlinked parent is
  refused; a grep of any generated Bash / stdout for the known plaintext finds
  nothing.

#### Phase 4: Declare all 49 names in keep `manifest.yml` (cross-repo: keep)
**Model:** sonnet + operator
- Enumerate all 46 `.age` + 3 aliases. syncthing certs/keys -> `secrets.file`
  with a destination path; everything else -> `secrets.env`.
- **Success criteria:** `secrets env` emits exactly the env-target count of lines;
  `secrets deploy --dry-run` lists every file-target name -> path; no `.age` in
  `.secrets/` is unaccounted for in `manifest.yml`.

#### Phase 5: Migrate the consumers -- shell + both live daemon units (cross-repo: dotfiles + live units)
**Model:** sonnet + operator
- Precondition (gated): `manifest secrets env --help` exits 0 on desk/lappy/mini.
- Point the shell at the new command:
  - `.zshenv:13` -> `eval "$(manifest -C ~/repos/scottidler/keep/manifest.yml secrets env)"`
- Point BOTH daemon sources at the new command, then REGENERATE the live units:
  - `sb/borg.yml:169` and `sb/cortex.yml:140` -> `manifest -C ~/repos/scottidler/keep/manifest.yml secrets env -f env`
  - re-run `sb borg daemon --install` / `sb cortex daemon --install` (or hand-edit
    the units), then `systemctl --user daemon-reload` + restart each daemon. The
    live `~/.config/systemd/user/{borg,cortex}.service` still contain
    `age decrypt <dir>` until this regenerates them; editing the yml alone does
    nothing.
- `.zshenv` is an always-on startup file: throwaway-launch tested BEFORE it lands
  (a startup-wiring change crashed the machine 2026-07-03;
  `no-untested-startup-config`).
- **Success criteria:** a fresh login shell has every env-target secret set AND,
  when `manifest` is broken/absent on PATH, still starts with the stderr banner
  shown (fail-soft + visible); borg and cortex restart with their expected env
  from `secrets env -f env`; the live unit files no longer contain
  `age decrypt <dir>` (grep the actual `~/.config/systemd/user/*.service`).

#### Phase 6: Retire the sweep usage + both poison lines, keep the single-file primitive (manifest code)
**Model:** opus
- Only AFTER a grep proves no consumer (shell, live units, cron, scripts) invokes
  `age decrypt <dir>`: remove BOTH poison placeholders (`age.rs:1131` in
  `render_exports`, `age.rs:1170` in `render_env`) so a per-secret failure skips
  with a `warn!` instead of emitting junk.
- **The single-file `age decrypt <file>` primitive stays and keeps its
  formatters.** `render_exports`/`render_env` back BOTH the single-file path and
  the directory sweep (`main.rs:299-303` routes both; `find_age_files` returns a
  lone file or a walked dir, `age.rs:72`). So this phase does NOT delete those
  functions -- it de-poisons them and retires only the *directory-argument-as-env
  -source usage* (no consumer passes a directory). The de-poisoned formatters
  keep serving `age decrypt <file>`.
- **Success criteria:** the string `manifest age command failed` exists nowhere in
  the codebase; `age decrypt <single-file.age>` still emits its line in both
  `-f export` and `-f env`; a per-secret decrypt failure in a single-file decrypt
  skips + `warn!`s with no placeholder; a tree-wide grep finds no
  `age decrypt <dir>` env-loading consumer.

## Acceptance Criteria

- [ ] `manifest secrets env` emits lines for `secrets.env` entries only; no
      `secrets.file` value ever appears in its output.
- [ ] A per-secret decrypt failure in `secrets env` emits nothing for that secret
      and a command-level failure emits zero stdout; the string
      `manifest age command failed` exists nowhere in the codebase (both former
      poison sites, `age.rs:1131` and `:1170`, gone).
- [ ] `secrets env -f env` output for a value containing a space, `"`, `$`, and a
      literal `'` loads cleanly via a real systemd `EnvironmentFile` parse (routed
      through `env_escape`, not `shell_escape`); `-f export` eval's cleanly in the
      shell.
- [ ] The single-file `age decrypt <file>` primitive still emits its line in both
      `-f export` and `-f env` after the sweep usage is retired.
- [ ] `manifest secrets deploy` writes each `secrets.file` entry to its path at
      `0600` atomically, with the temp file never broader than `0600` at any
      instant and the parent created at `0700`; a failed entry is reported, the
      rest deploy, exit is non-zero.
- [ ] `secrets deploy` does not follow a symlinked parent directory present at
      check time.
- [ ] A `secrets.file` value that is not a path string, an unknown key under
      `secrets:`, fails deserialization; a manifest with no `secrets:` block
      parses fine.
- [ ] Every consumer is migrated: `.zshenv`, and the LIVE
      `~/.config/systemd/user/{borg,cortex}.service` units (regenerated from
      `sb/borg.yml` + `sb/cortex.yml`) invoke `secrets env`; no `age decrypt <dir>`
      env-loading consumer remains anywhere (grep the live units, not just source).
- [ ] With `manifest` broken/absent on PATH, a new login shell still starts
      (fail-soft) AND the interactive stderr banner reports the drop (not silent);
      a non-interactive shell prints no banner.
- [ ] Before the shared `.zshenv` flip, `manifest secrets env --help` exits 0 on
      each of desk/lappy/mini (version gate).
- [ ] Every `.age` in keep `.secrets/` is declared in `manifest.yml`.

## Resolved Decisions

- **2026-07-17: Schema is two homogeneous blocks -- `env: Vec<String>` and
  `file: HashMap<name, path>`.** `env` reuses the package-block idiom; `file`
  reuses `LinkSpec.items`' exact type. Chosen over all alternatives on
  whole-solution coherence; both review panels converged independently. (Scott +
  panels.)
- **2026-07-17: Fail-closed is system-wide, not scoped.** All three consumers
  (shell, borg, cortex) migrate to `secrets env`; the dir-sweep-into-env is
  retired. A file secret then cannot reach any env. Blast radius is
  manifest + keep + dotfiles (three config lines); the daemon commands live in
  dotfiles config, NOT second-brain source, so `sb` is untouched. (Scott: "make
  this bulletproof.")
- **2026-07-17: File mode is a hardcoded `0600` const; per-file override
  deferred.** A per-entry mode would make `file` values maps and reintroduce the
  `link:`/`dirs` parse trap. No current secret needs a non-`0600` mode. (Scott.)
- **2026-07-17: Temp file is created at `0600`, not chmod-after.** The reused
  helpers leave a world-readable plaintext window (`encrypt_named` temp is
  `0644`; `generate_identity` chmods after write). For decrypted key material
  that is unacceptable. (Panel; "plaintext challenged on sight".)
- **2026-07-17: `secrets env` buffers and emits once (or nothing on error).** The
  fail-soft guarantee depends on it; made an explicit requirement, not an
  accident of current code. (Panel.)
- **2026-07-17: "Capture mode at encrypt time" rejected as unsound.** `age
  encrypt --name/--paste` has no source file (stdin/clipboard); git blob modes
  are only `100644`/`100755`, so git cannot carry `0600`. Mode must be a known
  value. (Panel.)
- **2026-07-17: `secrets deploy` is an explicit operator command, run once at
  machine setup**, not auto-run on every `manifest`. Documented in
  `keep/CLAUDE.md`. (Scott.)
- **2026-07-17: Deploy policies -- `create_dir_all` parent at `0700`; refuse a
  symlinked parent; per-file atomic, report-and-continue, non-zero exit.**
  (Scott + panel.)
- **2026-07-17: Transition by ship-order sequencing, not a version-fork.** Three
  machines, operator-controlled, one-time: install manifest everywhere, then flip
  the consumers. Rejected: version-fork in `.zshenv`, binary fallback, split
  stores, pinned old binary -- all leave transition cruft alive for a one-time
  event. (Scott: "I over-engineered a one-time event.")
- **2026-07-17: Symlink aliases listed by their own name in `secrets.env`.**
  Preserves the current independent per-alias env vars. (Research.)
- **2026-07-18: `-f env` routes through `env_escape`, `-f export` through
  `shell_escape`.** The revised draft mis-named `shell_escape` for both;
  `shell_escape` emits ANSI-C `$'...'` that systemd `EnvironmentFile` cannot
  parse. The new emit path selects the escaper by `DecryptFormat`, mirroring the
  existing `render_env`/`render_exports` split (`age.rs:106`/`:144`). (Panel R3.)
- **2026-07-18: The single-file `age decrypt <file>` primitive keeps its
  formatters; only the directory-as-env-source usage is retired.**
  `render_exports`/`render_env` back both paths (`main.rs:299-303`), so they are
  de-poisoned, not deleted. (Panel R3.)
- **2026-07-18: The live systemd units are first-class migration targets.** The
  daemon sweep executes from `~/.config/systemd/user/{borg,cortex}.service`
  (generated by `sb ... daemon --install`), not the dotfiles yml directly; Phase 5
  regenerates + restarts + probes them. `sb` Rust is still untouched. (Panel R3.)
- **2026-07-18: Machine-lag fails visibly, not silently.** On a command-level
  failure `secrets env` emits zero stdout (fail-soft) AND an interactive-only
  stderr banner naming the drop, plus a per-machine `secrets env --help` version
  gate before the `.zshenv` flip. Satisfies fail-soft without a silent secret
  drop. Chosen over accepting silent-empty-env. (Scott: "pick something safe.")
- **2026-07-18: Sweep retirement split into its own phase (Phase 6).** Consumer
  migration + live-unit regeneration (Phase 5) is committable and probeable
  independently of deleting the poison + dir-usage (Phase 6), which is gated on a
  grep proving no consumer remains. (Panel R3.)

## Alternatives Considered

### Schema alternatives (rejected; panel-ranked)
- **`{path, mode}` map value** -- explicit per-secret mode, but `file` values
  become maps and a mode is only useful once a non-default one exists (none do).
  Superseded by the `0600` const.
- **One map with a `target:` field** -- mixes scalar/map values (the `link:`/
  `dirs` trap); fail-closed rests on a typo-able string.
- **Encrypted mirror tree (position = destination)** -- env stays a list while
  file becomes a tree (two mechanisms); depends on the unsound capture-mode idea.
- **`.age` suffix inside `HOME/`** -- silently crosses keep's symlink-deploy vs
  copy-deploy boundary; a footgun. Panel disqualified.
- **Unified `Option<String>` map (None=env/Some=file)** -- relies on capture-mode;
  a null-valued YAML key is subtle; breaks the instant a non-`0600` mode is needed.

### Transition alternatives (rejected in favor of sequence-it)
- **Version-fork `.zshenv`** (check `manifest --version`, new|old) -- survives
  per-machine lag but leaves transition logic in `.zshenv` forever.
- **Binary fallback** (new command falls back to sweep) -- fallback code lives in
  the binary; must be removed later.
- **Split stores** (env `.age` and file `.age` in separate folders) -- fixes the
  leak with no fork, but changes the storage layout (breaks "stays in
  `.secrets/`").
- **Pinned old binary** -- an extra artifact to babysit.
All rejected: the migration is one-time across three operator-controlled
machines, so sequencing beats permanent transition machinery.

### Scope alternatives (rejected)
- **Keep the blanket sweep, add a file step only** -- leaves TLS keys leaking
  into three envs; secrets still undeclared.
- **Defer the daemon migration** -- would leave fail-closed only partial; Scott
  chose bulletproof (migrate all three now), and the cost is cheap (dotfiles
  config).
- **SOPS / agenix** -- a second secrets toolchain; manifest already wraps `age`.

## Technical Considerations

### Dependencies
- Internal: `age.rs` (`filename_to_var`, `shell_escape`, `resolve_identity`,
  `encrypt_named` atomic-write skeleton, `generate_identity` chmod idiom),
  `cli.rs` (`DecryptFormat`), `config.rs` (`expand_tilde`, `ManifestSpec`).
- External: existing `age` crate (`0.11`, `armor`,`ssh`). No new deps.

### Performance
- `.zshenv` runs in every shell (~24ms today). `secrets env` decrypts the same
  env set; expected comparable. Measure in Phase 5.

### Security
- Fail-closed, system-wide: after migration + sweep retirement, no consumer can
  emit a file secret into an env.
- `secrets deploy` writes secret bytes only to the declared path at `0600`,
  atomically, with the temp never broader than `0600` from creation (`mode &
  ~umask` can only narrow), and does not follow a symlinked parent present at
  check time (`symlink_metadata`, not `canonicalize().is_symlink()`, which is
  always false). A check-to-write TOCTOU remains, out of scope for this threat
  model. Never to stdout, a Bash string, or a temp outside the dir.
- The passphrase-less signing key is safe here precisely because it is a file at
  `0600`, never an env var, and is signing-only (cannot auth/push).

### Testing Strategy
- Schema: round-trips; map-valued `file` entry, unknown keys, and no-`secrets:`
  all handled.
- Emit: allowlist proof; per-secret failure emits nothing; command-level failure
  zero stdout; no placeholder string in the binary; name/value SET matches legacy.
- Deploy: `0600` via metadata; temp-mode-at-creation asserted; mid-batch failure
  leaves no partial for that entry, deploys the rest; symlinked parent refused;
  plaintext never in stdout/Bash.
- Tests must bite: break the code, prove the negative tests fail.

### Rollout Plan
- manifest (schema + subcommands) installed AND `secrets env --help`-proven on
  desk/lappy/mini -> keep declares all 49 -> migrate `.zshenv` + regenerate the
  live `borg.service`/`cortex.service` units from `sb/borg.yml` + `sb/cortex.yml`
  -> (separately, Phase 6) retire the sweep usage + both poison lines. `.zshenv`
  throwaway-launch tested last.

## Risks and Mitigations

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| `.zshenv` swap breaks every shell | Low | High | Buffer-then-emit + fail-soft (exit 0, empty on error); throwaway-launch test; it is the last step |
| A consumer flips before manifest is installed on that machine | Low | High | Per-machine `secrets env --help` version gate before the shared `.zshenv` flip; interactive stderr banner makes any miss visible (not silent); 3 operator-controlled machines |
| A secret silently drops from an env after migration | Med | Med | Phase 4/5 criteria assert emitted set == expected; every `.age` accounted for; borg/cortex restart-check; command-level failure is visible via stderr banner, never silent |
| Live daemon units keep sweeping after only the yml is edited | Med | High | Phase 5 regenerates the units (`sb ... daemon --install`) + `daemon-reload` + restart + greps the live `.service` files; AC checks the units, not just source |
| Decrypted plaintext world-readable on disk | Low | High | Temp created at `0600`; refuse symlinked parent; atomic rename |
| Secret bytes leak via Bash/stdout | Low | High | File lane is native Rust, never the render path; grep-for-plaintext test |
| A future secret needs a non-`0600` mode | Low | Med | Deferred override via a separate `file-modes:` map, designed when it lands |

## Open Questions

- [ ] None. Schema, mode, deploy mechanism + policies, alias mapping, the
      consumers (shell + both live daemon units), the sweep retirement, the
      `-f env` escaping, the single-file primitive, and the machine-lag guard are
      all resolved above. The third (revised-full-doc) panel's findings are folded
      in; nothing left open.

## References
- Research brief + two review panels (this session).
- Consumers (source): `dotfiles/HOME/.zshenv:13`,
  `dotfiles/HOME/.config/sb/borg.yml:169`, `dotfiles/HOME/.config/sb/cortex.yml:140`.
- Consumers (live executable units): `~/.config/systemd/user/borg.service:10`,
  `~/.config/systemd/user/cortex.service:7` (`ExecStartPre` sweep +
  `EnvironmentFile`), generated by `sb {borg,cortex} daemon --install`
  (`second-brain/main/borg/src/service.rs:211`, `cortex/src/daemon.rs:860`).
- `src/config.rs:44-93` (ManifestSpec), `:99-103` (link/dirs), `:107` (LinkSpec),
  `:110` (package idiom).
- `src/age.rs:80` (unsorted walk), `:106` (env_escape), `:144` (shell_escape),
  `:775` (atomic write), `:950` (chmod), `:1094`/`:1131` (render_exports/poison),
  `:1139`/`:1170` (render_env/poison).
- `src/cli.rs:279` (DecryptFormat), `src/main.rs:299-303` (single-file + dir both
  route through render_exports/render_env).
- `src/main.rs:360,387` (age encrypt --name/--paste have no source file).
- Prior art: `docs/design/2026-01-24-manifest-age-subcommand.md`,
  `2026-04-17-link-dirs-subfield.md`.

## Addendum: parked operator hygiene (not this design)
- `.secrets/datadog-api-key.txt` / `datadog-app-key.txt` are plaintext at rest
  (not `.age`). Operator cleanup, separate.
- `keep/CLAUDE.md` claims secrets load via `~/.shell-exports.d/secrets.env`; the
  real mechanism is the `.zshenv:13` eval. Fix that doc drift when Phase 5 lands.
- `scottidler/claude` `settings.json:708` allowlists `Bash(manifest age decrypt:*)`;
  harmless, update if the single-file primitive's surface changes.

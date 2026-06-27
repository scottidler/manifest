# Design Document: First-class clipboard-to-`.age` encryption

**Author:** Scott Idler (design by Claude)
**Date:** 2026-06-27
**Status:** Implemented
**Review Passes Completed:** 5/5 + cross-model panel (Architect/Gemini, Staff Engineer/Codex)

## Summary

Add two argv-safe ways to encrypt a named secret into a correctly-named `.age` file:
`manifest age encrypt --name NAME` (value from stdin) and `manifest age encrypt --paste NAME`
(value from the system clipboard). Both write `<name>.age` via the existing naming
convention, never expose the plaintext in argv, never write the clipboard (except via an
explicit opt-in `--clear-clipboard`), and verify the result by a true decrypt round-trip
rather than a length heuristic.

## Problem Statement

### Background

`manifest age encrypt` today accepts a `Vec<String>` of inputs and classifies each at
runtime into exactly three modes (`src/main.rs:264-308`):

- `-` -> read stdin, write **raw ciphertext to stdout** (caller redirects and names the file by hand)
- an existing path -> encrypt that file
- `KEY=VAL` -> encrypt `VAL`, write `var_to_filename(KEY)` (e.g. `drata-readonly-api-key.age`)

The original 2026-01-24 design had a `--name` + stdin interface
(`echo -n secret | manifest age encrypt --name my-secret`). The 2026-03-08 restructure
(`docs/design/2026-03-08-age-subcommand-restructure.md`) replaced `-e`/`-d` with
`encrypt`/`decrypt`, introduced `KEY=VAL`, and **dropped `--name`**, reducing stdin to
ciphertext-on-stdout. The "value from elsewhere -> correctly-named file" capability was
lost in that restructure.

### Problem

There is no path from "a secret value I have (in my clipboard, on stdin) " to "exactly one
correctly-named `.age` file" that does not leak the plaintext. The only way to produce a
named file today is `KEY=VAL`, which puts the plaintext in the **process argument list**
(visible in `ps` / `/proc/<pid>/cmdline`) for the life of the call. The only argv-safe
input (stdin `-`) does not name a file. For a secrets tool, the most common secret-entry
workflow is both multi-step and unsafe.

The incident that triggered this (see `docs/design/2026-06-27-clipboard-encrypt-handoff.md`)
also exposed two adjacent failures: a scratchpad `printf ... | wl-copy` **overwrote the real
clipboard** so the encrypt captured a placeholder, and "verification" was a 24-char length
check that passed on the wrong value; and the file was written to an invented
`~/.config/secrets/` instead of the real store `~/repos/scottidler/secrets/.secrets/`.

### Goals

- One command, value from clipboard -> correctly-named `.age` file (`--paste NAME`).
- One orthogonal primitive, value from stdin -> correctly-named `.age` file (`--name NAME`).
- Plaintext never appears in argv.
- The command never writes the clipboard, unless the user opts in with `--clear-clipboard`.
- Cross-platform clipboard read (Wayland / X11 / macOS), clear error if unavailable.
- Honest verification: decrypt the file just written and compare byte-for-byte to the input.
- Sensible destination: auto-target the configured secrets store; never silently default to `.` or invent a directory.
- `--force` required to overwrite an existing `<name>.age`.

### Non-Goals

- Replacing the file or stdin-`-` modes. They stay as-is. (`KEY=VAL` is *not* fully untouched -
  it gains the atomic write + `--force` overwrite guard; see below.)
- Editing/rotating an existing encrypted value in place.
- GUI clipboard managers, clipboard history, or selecting a non-default selection buffer (primary/secondary).
- Decryption-side changes (`manifest age decrypt` is untouched).

## Proposed Solution

### Overview

Add four optional fields to the `encrypt` subcommand: `--name <NAME>`, `--paste <NAME>`,
`--force`, and `--clear-clipboard`. `--name`, `--paste`, and the positional `inputs` are
mutually exclusive input sources (exactly one required), enforced by a clap `ArgGroup`.

- `--name NAME` reads the secret from **stdin**, encrypts it, and writes `var_to_filename(NAME)`.
  This is the orthogonal primitive.
- `--paste NAME` reads the secret from the **clipboard** (read-only), then takes the same path
  as `--name`. It is sugar over "read clipboard | --name".

Both reuse the existing in-process `age::encrypt` (`src/age.rs:16`) and the existing
`var_to_filename` naming contract (`src/age.rs:95`). After writing, the command decrypts the
file with a resolved identity and asserts the plaintext is byte-equal to what it read.

### Architecture

```
                 ┌──────────────────────────── input sources (mutually exclusive) ────────────┐
                 │  positional inputs (file | KEY=VAL | -)   --name NAME (stdin)   --paste NAME │
                 └───────────────┬─────────────────────────────────┬──────────────────────────┘
                                 │                                 │
              (existing modes, unchanged)            read_clipboard()  read_stdin()
                                 │                          │           │
                                 │                          └─── strip trailing newline ───┐
                                 │                                                          │
                                 │                                      ensure non-empty (else error)
                                 │                                                          │
                                 ▼                                                          ▼
                          var_to_filename(NAME) ──► resolve output dir ──► encrypt(bytes, recipient)
                                                          │                        │
                                    (-o > config secrets-store > error; no "." for paste/name)
                                                          │                        │
                                              refuse-to-invent / --force guard      │
                                                          └──────────┬─────────────┘
                                                                     ▼
                                                            write <dir>/<name>.age
                                                                     │
                                              honest round-trip verify (decrypt, byte-equal)
                                                                     │
                                              optional: --clear-clipboard (opt-in write)
```

### Data Model

No persisted data structures change. One new optional config field on `ManifestSpec`
(`src/config.rs`) to support auto-targeting the secrets store:

```yaml
# manifest.yml
secrets-store: ~/repos/scottidler/secrets/.secrets   # optional; tilde-expanded
```

- `ManifestSpec` does **not** use a container `rename_all`; it renames each kebab field
  individually (`config.rs:40` `uv-tool`, `:49` `git-crypt`). The new field must therefore be
  declared `#[serde(rename = "secrets-store")] secrets_store: Option<PathBuf>`, or it silently
  won't load from a `secrets-store:` key.
- `PathBuf` deserialization does **not** expand `~`, and config loading is a bare `from_reader`
  with no post-processing (`config.rs:11-13`). So `secrets-store: ~/secrets` would resolve to a
  literal `~` directory. The field must be tilde-expanded after load (a small post-deserialize
  pass, matching however other manifest paths are normalized).
- Absent -> no auto-target. For `--name`/`--paste` this means `-o` is required (else error).
  **Legacy positional modes do not consume `secrets-store` at all** (see "Output directory
  resolution" and Alternative 4) - their `.` default is unchanged.

### API Design

CLI (`src/cli.rs`, `AgeAction::Encrypt`):

```rust
Encrypt {
    /// File paths, KEY=VAL pairs, or "-" for stdin (existing modes)
    #[arg(num_args = 1.., group = "encrypt-input")]
    inputs: Vec<String>,

    /// Read the secret value from stdin and write <name>.age
    #[arg(long, value_name = "NAME", group = "encrypt-input")]
    name: Option<String>,

    /// Read the secret value from the system clipboard and write <name>.age
    #[arg(long, value_name = "NAME", group = "encrypt-input")]
    paste: Option<String>,

    /// Overwrite an existing <name>.age (otherwise refuse)
    #[arg(long)]
    force: bool,

    /// After a successful write, clear the system clipboard (opt-in)
    #[arg(long)]
    clear_clipboard: bool,

    /// Output directory. See "Output directory resolution" for per-mode defaults.
    #[arg(short = 'o', long = "output-dir")]
    output_dir: Option<PathBuf>,
}
```

`ArgGroup` `encrypt-input` is `required(true)`, `multiple(false)` -> exactly one of
`inputs` / `--name` / `--paste`.

New functions in `src/age.rs`:

```rust
/// Read the clipboard read-only via the first available tool, strip one trailing
/// newline, error on empty or no-tool-found. Never writes the clipboard.
fn read_clipboard() -> Result<Vec<u8>>;

/// Clear the system clipboard (only called for --clear-clipboard).
fn clear_clipboard() -> Result<()>;

/// Encrypt `plaintext` to `<output_dir>/var_to_filename(name)`, honoring `force`.
/// Returns the written path.
fn encrypt_named(name: &str, plaintext: &[u8], recipient: &dyn Recipient,
                 output_dir: &Path, force: bool) -> Result<PathBuf>;

/// Decrypt `path` with `identity` and assert it equals `expected`. Skips with a
/// warning if no identity is resolvable (e.g. only a bare `-r` recipient was given).
fn verify_roundtrip(path: &Path, expected: &[u8], identity: Option<&Path>) -> Result<()>;

/// Strip a single trailing "\n" or "\r\n". Pure, unit-testable.
fn strip_trailing_newline(bytes: Vec<u8>) -> Vec<u8>;

/// Reject empty names and names containing path separators or "."/"..". Pure, unit-testable.
fn validate_name(name: &str) -> Result<()>;
```

### Clipboard read strategy

Shell out to read-only tools; do **not** add a clipboard crate.

- Rationale: a crate such as `arboard` links both get and set (and on X11 keeps a process
  alive to serve the selection). The incident's root failure was touching the clipboard write
  side; shelling to read-only invocations guarantees no write path is even linked.
- Selection must be **session-aware, with runtime fall-through** - presence on `PATH`
  (`check_hash`/`command -v`, `src/cli.rs:7-24`) is necessary but not sufficient. `wl-paste` can
  be installed on an X11 box and fail because `$WAYLAND_DISPLAY` is unset. So: prefer the tool
  matching the live session (`$WAYLAND_DISPLAY` -> `wl-paste`; `$DISPLAY` -> `xclip`/`xsel`;
  macOS -> `pbpaste`), and if the chosen tool **exits non-zero**, fall through to the next
  installed candidate rather than aborting. Candidate read-only invocations:
  1. `wl-paste -n` (Wayland; `-n` already omits the trailing newline)
  2. `xclip -selection clipboard -o` (X11)
  3. `xsel -b -o` (X11)
  4. `pbpaste` (macOS)
- If none is installed -> `eyre!` listing the tools tried. If candidates exist but all exit
  non-zero -> `eyre!` surfacing the last tool's stderr (do **not** treat a tool error as an
  empty clipboard).
- **Timeout + size cap:** a clipboard tool can hang (e.g. waiting on a dead compositor) or
  return many MB. Run with a wall-clock timeout and cap captured bytes; exceed either ->
  `eyre!`. In a headless/SSH session (`$WAYLAND_DISPLAY` and `$DISPLAY` both unset, not macOS),
  fail fast with an actionable error rather than spawning a tool that will hang.
- **Log the chosen tool** at `debug!` (`read_clipboard: tool=wl-paste bytes=N`) so an "empty
  clipboard" error is diagnosable - which binary ran, how many bytes it returned. Length only,
  never the bytes.
- Strip a single trailing `\n`/`\r\n` from captured bytes (belt-and-suspenders even with
  `wl-paste -n`). Do **not** strip spaces, tabs, or interior newlines - they may be part of the
  secret. (Multi-newline trailing junk from a sloppy selection is *not* stripped beyond one; a
  paste that picks up extra blank lines is the user's to fix, and over-stripping risks mangling
  a secret. Decision: strip-exactly-one.)
- Empty after strip -> `eyre!`, write nothing.
- `--clear-clipboard` write forms (only when opted in): `wl-copy --clear`, `xclip -selection clipboard -i </dev/null`, `xsel -bc`, `pbcopy </dev/null`. A failure to clear is a `warn!`, not fatal - the file is already written and verified.

### Name validation

`var_to_filename` (`src/age.rs:95`) only lowercases and swaps `_`->`-`; it does **not**
sanitize path separators. So `--paste a/b` would yield `a/b.age` and `--paste ../../x` would
escape the output dir - a path-traversal write. Before calling `var_to_filename`, validate the
name:

- Reject an empty name (`--paste ""` would otherwise produce `.age`).
- Reject any name containing `/`, `\`, or a path component (`.`/`..`). A secret name is a bare
  identifier (`DRATA_READONLY_API_KEY`), never a path.
- `eyre!` with a clear message on violation; write nothing.

This guard lives in `encrypt_named` (or a small `validate_name` helper) so both `--name` and
`--paste` get it for free.

### Stdin handling for `--name`

`--name` reads the secret from stdin. If stdin is an interactive TTY (nothing piped), a naive
read blocks forever. Detect a TTY (`std::io::IsTerminal`) and `eyre!` immediately with a
fix-it message ("pipe a value into --name, or use --paste"). A single trailing `\n`/`\r\n` is
stripped from stdin input too, matching clipboard semantics; interior newlines and a secret's
own trailing spaces are preserved.

### Output directory resolution

`output_dir` is now `Option<PathBuf>` with **no clap `default_value`** - there is no `"."`
constant baked into the flag. Resolution happens in the `Encrypt` handler and differs by
input mode:

**New `--name` / `--paste` paths** (highest first):

1. `-o DIR` if given explicitly.
2. `secrets-store` from the loaded `ManifestSpec`, tilde-expanded.
3. **Neither set -> error.** There is no default const to fall back on, so the command refuses
   rather than dumping a secret into the current directory: `eyre!("no output directory: pass
   -o DIR or set secrets-store in manifest.yml")`. This closes the wrong-destination class of
   bug at its root (the `~/.config/secrets/` detour, and silent writes to `.`).

**Legacy positional modes** (`KEY=VAL`, file, `-`): they do *not* consume `secrets-store` and
keep today's `.` default *destination*. `KEY=VAL`'s **write semantics** do change, though - it
is rerouted through `encrypt_named` and gains the atomic write + `--force` guard (see the
Output-behavior matrix). The reason `secrets-store` is still *not* applied to `KEY=VAL` is no
longer the missing guard (that is now fixed) but that auto-targeting would silently change
*where* existing `KEY=VAL` scripts write; keeping the destination at `.` avoids that surprise
(see Alternative 4).

After resolution (new paths), **refuse to write into a directory that does not already exist**
-> `eyre!` telling the user to create it or pass `-o`. This is what would have caught the
`~/.config/secrets/` detour even when a path was supplied.

**Lazy config load (do not reorder globally).** Today every age command returns at
`src/main.rs:330`, *before* config load (`:339`) and before `debug!("Parsed CLI arguments ...")`
(`:337`). Moving config load ahead of the age dispatch globally would make `age --keygen`,
`--public-key`, `decrypt`, stdin->stdout, and explicit-`-o` encrypt all newly fail when
`manifest.yml` is missing or malformed. Therefore: load config **lazily, only inside the
`--name`/`--paste` branch, and only after confirming `-o` was not given**. Do **not** relocate
the `Parsed CLI arguments` debug log ahead of the age dispatch - it would log legacy `KEY=VAL`
plaintext (the log file is not argv, but it is still plaintext-at-rest).

### Atomic write + honest verification

The naive "write the target, then delete it on mismatch" is itself a footgun: a `--force`
rotation that fails verification would clobber the previously-working secret and then delete the
bad one, leaving the user with **nothing**. The write must be atomic and verified *before* it
becomes the target:

1. Write ciphertext to a **temp file in the target's own directory** (same filesystem, so the
   rename is atomic), e.g. `<dir>/.<name>.age.tmp-<pid>`. `flush`/`sync_all` it.
2. **Verify the temp file** (see outcomes below).
3. Overwrite policy:
   - **no `--force`:** the rename must fail if the target exists. Use create-new / `O_EXCL`
     semantics on the final name (or check-then-rename is acceptable here only because a losing
     race fails closed); do **not** pre-check with `exists()` and a separate write (TOCTOU).
   - **`--force`:** atomic-rename the temp over the existing target. The old secret survives
     until the instant the verified new one replaces it.
4. On any failure, remove the **temp** file; the target is never touched.

If the target is an existing symlink (the secrets repo allows symlinks - `secrets/CLAUDE.md`),
an atomic rename **replaces the symlink with a regular file**; document this so it is intentional,
not surprising.

Verification of the temp file via `age::resolve_identity` (`src/age.rs:356`) + `decrypt_file`
has three outcomes:

1. **Decrypt succeeds, bytes match** -> rename into place. Done.
2. **Decrypt succeeds, bytes differ** -> genuine corruption. `eyre!`, remove the temp, never
   rename. This is the case the length-heuristic bug missed.
3. **Verification not possible** -> no identity resolvable, *or* an identity resolved but cannot
   decrypt this recipient's file. This is **not** corruption. `warn!` **and `eprintln!` to
   stderr** (a `warn!` alone is invisible - see Logging), explain why, and rename the file into
   place anyway. Note the residual ambiguity (finding #10): a decrypt failure when an identity
   *does* match the recipient is technically corruption but is indistinguishable here from a
   mismatched identity; we fail safe toward keeping the file and surfacing the skip loudly.

### Output-behavior matrix

Folding the new paths into output-dir resolution must not change the stdout-only modes. Explicit
contract:

| Mode | Output dir resolution | Writes | `--force` | `secrets-store` |
|------|----------------------|--------|-----------|-----------------|
| `--name NAME` | `-o > secrets-store > error` | `<dir>/<name>.age` (atomic) | guarded | yes |
| `--paste NAME` | `-o > secrets-store > error` | `<dir>/<name>.age` (atomic) | guarded | yes |
| `KEY=VAL` | `-o > .` (no secrets-store) | `<dir>/<filename>.age` (atomic) | **guarded (new)** | no |
| file path | (unchanged) | ciphertext to **stdout** | n/a | no |
| `-` (stdin) | (unchanged) | ciphertext to **stdout** | n/a | no |

`KEY=VAL` now routes through `encrypt_named`, so it inherits the atomic temp-write+rename, the
`--force` overwrite guard, `validate_name`, the refuse-to-invent guard, and the same round-trip
verification (verify-before-rename, warn-and-keep when no identity). This is a deliberate
behavior change: `KEY=VAL` no longer silently clobbers an existing `<filename>.age` - it errors
unless `--force` is given (see Risks). Its **default output dir stays `.`** (it does *not*
consume `secrets-store`); auto-targeting `KEY=VAL` at the secrets store would silently relocate
where existing scripts write, which is out of scope here.

The "refuse to write into a non-existent directory" guard applies **only** to the modes that
write into a directory (`--name`, `--paste`, `KEY=VAL`); it must not affect the stdout-only
file/`-` modes.

### Logging

Per `logging.md`: function-level `debug!` entry/exit with length-only secret payloads. Critical
caveat verified in review: this repo routes `env_logger` to the log file
(`~/.local/share/manifest/logs/manifest.log`, `src/main.rs:195-203`), so a `warn!` **never
reaches the user's terminal**. Every *safety-downgrade* message must therefore also `eprintln!`
to stderr (mirroring the existing identity-fallback notice at `src/age.rs:376`):

- verification skipped ("kept an unverified file because no usable identity") -> `warn!` + `eprintln!`
- `--clear-clipboard` failed -> `warn!` + `eprintln!`

### Implementation Plan

#### Phase 1: CLI surface
**Model:** sonnet
- Add `--name`, `--paste`, `--force`, `--clear-clipboard` fields and the `encrypt-input`
  `ArgGroup` to `AgeAction::Encrypt` (`src/cli.rs:233-242`). `ArgGroup` is not yet imported
  (`src/cli.rs:3` brings in only `ArgAction, Parser, Subcommand`) - add it. Change `output_dir`
  to `Option<PathBuf>` (drop `default_value = "."`).
- Thread the new fields through the `Encrypt` match arm signature in
  `handle_age_command` (`src/main.rs:257`).
- **Do not assume the `ArgGroup` fully enforces exclusivity** between a `Vec<String>` positional
  and the flags - clap is historically fragile here. Add a manual validation step in the handler
  (mirroring the mixed-mode error at `src/main.rs:264-278`) and back it with CLI parse tests in
  Phase 7.
- Compile-only; no behavior yet. Existing positional modes still work unchanged.

#### Phase 2: Clipboard read abstraction
**Model:** opus
- Implement `read_clipboard()` and `strip_trailing_newline()` in `src/age.rs`.
- Cascading read-only tool detection, single-newline strip, empty -> error, no-tool -> error.
- Function-level `debug!` entry/exit logging with **length-only** payload (never the bytes).
- Factor byte-processing into the pure `strip_trailing_newline` so it is unit-testable
  independent of a real selection.

#### Phase 3: `--name` stdin primitive
**Model:** sonnet
- Implement `validate_name(...)` and `encrypt_named(...)` in `src/age.rs` (reuse
  `var_to_filename` + `encrypt`). `encrypt_named` calls `validate_name` first, then writes via
  the **atomic temp-file-then-rename** flow (see "Atomic write + honest verification"): write
  `<dir>/.<name>.age.tmp-<pid>`, `sync_all`, and rename into place. The `--force` policy is the
  rename rule (no-force fails-if-exists via `O_EXCL`-style create; force renames over), and the
  refuse-to-invent (dir-must-exist) guard runs first - all pure functions of
  `(output_dir, name, force)`, needing no config.
- In the `Encrypt` handler, when `--name` is set: guard against an interactive TTY
  (`IsTerminal`), read stdin to bytes, strip trailing newline, `encrypt_named`. Reuse/refactor
  the stdin read used by `encrypt_stdin` (`src/age.rs:35`).

#### Phase 4: `--paste` sugar + mutual exclusion + verification
**Model:** opus
- Wire `--paste NAME` as `read_clipboard()` -> same path as `--name`.
- Finalize the manual exclusivity validation from Phase 1 (the `ArgGroup` is a backstop, not the
  sole gate).
- Implement `verify_roundtrip(...)` and **insert it into the atomic flow before the rename**:
  verify the temp file, then rename only on outcomes 1/3, `eyre!`+remove-temp on outcome 2. The
  skip path (outcome 3) emits `warn!` **and** `eprintln!` (see Logging). This makes the
  "never rename an unverified-corrupt file" invariant structural.

#### Phase 5: Output-dir resolution + config field
**Model:** sonnet
- Add `#[serde(rename = "secrets-store")] secrets_store: Option<PathBuf>` to `ManifestSpec`
  (`src/config.rs`) - explicit per-field rename, since there is no container `rename_all`.
- Add a post-load tilde-expansion step for the field (`from_reader` does not expand `~`).
- **Lazy load:** do not reorder the global config load. Load config (and read `secrets_store`)
  only inside the `--name`/`--paste` branch, and only when `-o` was absent, so missing/malformed
  `manifest.yml` cannot break `--keygen`, `--public-key`, `decrypt`, or `-o`-explicit encrypts.
- Resolve only the new paths: `-o > secrets-store > error`. **Do not** apply `secrets-store` to
  legacy positional modes; they keep their existing `.` default untouched (see Alternative 4).
- **Reroute `KEY=VAL` through `encrypt_named`** (`src/main.rs:294-299`): replace the bare
  `std::fs::write` with the `encrypt_named` call so `KEY=VAL` gains the atomic write, the
  `--force` overwrite guard, `validate_name`, refuse-to-invent, and round-trip verification.
  Pass the resolved `KEY` (already turned into a filename via `var_to_filename`) as the name and
  the existing `.`-or-`-o` directory. This is the deliberate `KEY=VAL` behavior change (errors on
  existing file without `--force`).

#### Phase 6: `--clear-clipboard`
**Model:** sonnet
- Implement `clear_clipboard()` (opt-in write forms) and call it only on
  `--clear-clipboard` after a successful, verified write.

#### Phase 7: Tests + docs
**Model:** sonnet
- Inline `#[cfg(test)] mod tests` in `src/age.rs` matching `test_encrypt_kv_roundtrip`
  (`src/age.rs:679`): `encrypt_named` round-trip (encrypt known bytes, decrypt, assert
  byte-equal); `strip_trailing_newline` cases (`\n`, `\r\n`, none, interior preserved, trailing
  spaces preserved); empty-input -> error; `validate_name` rejections (empty, `a/b`, `../x`,
  backslash); force/overwrite guard (exists+no-force -> error, exists+force -> overwrite);
  refuse-to-invent guard (missing dir -> error); `verify_roundtrip` three outcomes (match ->
  rename; mismatch -> error + temp removed + target untouched; undecryptable -> warn + rename);
  **atomic-overwrite safety** (`--force` over an existing file that then fails verification ->
  the original file is preserved, not destroyed - the regression test for CRITICAL #1).
- CLI parse tests for the `ArgGroup` + manual validation: no input -> error; `--name` only ->
  ok; `--paste` only -> ok; `--name` + positional -> error; `--name` + `--paste` -> error;
  multi-value positional -> still works.
- `secrets-store` deserialization test: a `secrets-store: ~/x` key loads into `secrets_store`
  and tilde-expands.
- `KEY=VAL` regression: existing `<filename>.age` + no `--force` -> error (not silent clobber);
  with `--force` -> overwrites atomically; destination still defaults to `.` (not `secrets-store`).
- Update `CLAUDE.md` quick reference and any age-subcommand docs to mention `--name`/`--paste`.

## Alternatives Considered

### Alternative 1: Dedicated `manifest age paste NAME` subcommand
- **Description:** A standalone subcommand instead of a flag on `encrypt`.
- **Pros:** More discoverable in `--help`; cleaner separation.
- **Cons:** Splits encryption across two subcommands; duplicates recipient/identity/`-o` plumbing.
- **Why not chosen:** The handoff and the 2026-03-08 restructure both lean "flag on `encrypt`" for consistency.

### Alternative 2: `arboard` (or other clipboard crate)
- **Description:** Read the clipboard via a Rust crate instead of shelling out.
- **Pros:** No external tool dependency; one code path across platforms.
- **Cons:** Links the clipboard **write** API into the binary; on X11 must keep a process alive to serve the selection. Directly contradicts the "never touch the write side" requirement.
- **Why not chosen:** Read-only shell-out is provably free of a write path and matches the repo's existing tool-detection idiom.

### Alternative 3: Auto-discover the secrets store by walking the repo tree
- **Description:** Derive `~/repos/scottidler/secrets/.secrets/` from `discover_repo_root`.
- **Pros:** Zero config.
- **Cons:** `discover_repo_root` (`src/config.rs:212`) finds the *dotfiles* repo (parent of `HOME/`), not the secrets repo; there is no existing resolver for the secrets store, and hard-coding a personal path is wrong for a shared tool.
- **Why not chosen:** An explicit, optional `secrets-store` config field is correct, portable, and discoverable.

### Alternative 4: Apply `secrets-store` auto-targeting to `KEY=VAL` too
- **Description:** Also default `KEY=VAL`'s output dir to `secrets-store` (full uniform `-o > secrets-store > .` resolution), not just the overwrite guard.
- **Context:** The panel (finding #4) flagged that auto-targeting `KEY=VAL` was unsafe *because it had no `--force` guard*. That objection is now removed - per Scott's direction, `KEY=VAL` routes through `encrypt_named` and gains the atomic write + `--force` guard. So extending `secrets-store` to it would be *safe*.
- **Pros:** One uniform resolution rule across all directory-writing modes.
- **Cons:** It silently changes **where** existing `KEY=VAL` invocations write (today `.`), which can surprise scripts that rely on the current directory. The overwrite guard protects against clobbering, but not against a changed destination.
- **Why not chosen (for now):** Keep the *destination* behavior of `KEY=VAL` unchanged (default `.`) while still hardening it with the `--force` guard. `secrets-store` auto-targeting stays scoped to the new `--name`/`--paste` paths. This can be revisited cheaply later since the guard is now in place.

## Technical Considerations

### Dependencies
- No new Rust crates. Clipboard read/write is shell-out to `wl-paste`/`xclip`/`xsel`/`pbpaste` (and `wl-copy`/`pbcopy` for the opt-in clear).
- Reuses `age` crate (`Cargo.toml:13`), `eyre`, `tempfile` (tests).

### Performance
- Negligible: one subprocess for the clipboard read, one in-process encrypt, one in-process decrypt for verification.

### Security
- Plaintext never enters argv (the core fix). The secret lives only in process memory and stdin/clipboard.
- Clipboard read is read-only by default; the only write path (`--clear-clipboard`) is opt-in.
- Logging is length-only for the secret payload (per `logging.md` sensitive-payload rule).
- Honest round-trip verification prevents "verified" garbage; bad artifacts are removed on mismatch.

### Testing Strategy
- Inline unit tests in `src/age.rs`. The clipboard shell-out is not unit-tested directly
  (depends on a live selection); the byte-processing (`strip_trailing_newline`, empty check)
  is factored into pure, tested helpers, and `encrypt_named` + `verify_roundtrip` are tested
  end-to-end with `tempfile` and a generated identity.
- Manual acceptance per the handoff checklist (argv inspection via `ps`/`/proc`, real Wayland/X11 clipboards).

### Rollout Plan
- Additive, backward-compatible. Ship behind no flag; bump version; existing modes unchanged.

## Risks and Mitigations

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| Clipboard tool missing in CI/headless | Med | Low | Clear error listing tried tools; clipboard path not exercised in unit tests |
| `--clear-clipboard` reintroduces a write footgun | Low | Med | Strictly opt-in; never default; documented |
| Verification silently skipped (pubkey-only) leaves unverified file | Med | Med | Loud `warn!`; document; encourage identity availability |
| Verify deletes a good file because identity can't decrypt the recipient | Low | High | Three-outcome `verify_roundtrip`: only a successful-decrypt byte-mismatch deletes; undecryptable warns and keeps |
| `--force` rotation fails verification and destroys the prior good secret | Low | High | Atomic temp-write + verify-temp + rename; the original is replaced only by a verified file, never deleted first (CRITICAL #1) |
| `secrets-store` silently fails to load (no container `rename_all`; no `~` expansion) | Med | Med | Explicit `#[serde(rename)]` + post-load tilde-expansion; deserialization test (CRITICAL #2) |
| Eager config load breaks `--keygen`/`--public-key`/`decrypt` on missing `manifest.yml` | Med | High | Lazy load only in the `--name`/`--paste` branch, only when `-o` absent (CRITICAL #3) |
| `warn!` never reaches the terminal (logs to file) so a safety downgrade is silent | High | Med | Safety-downgrade paths also `eprintln!` to stderr (MAJOR #5) |
| Hung/huge clipboard tool blocks or floods the process | Low | Med | Wall-clock timeout + byte cap; headless fail-fast (MINOR #8) |
| Path-traversal / bad name (`--paste ../x`, `a/b`) escapes the output dir | Low | High | `validate_name` rejects separators, `.`/`..`, and empty before any filesystem write |
| `--name` on an interactive TTY blocks forever | Med | Low | `IsTerminal` guard errors immediately with a fix-it message |
| `wl-paste` installed but session is X11 -> false "empty"/error | Med | Med | Session-aware selection + fall-through on non-zero exit; tool errors never read as empty |
| `secrets-store` points at a non-existent dir | Low | Med | Refuse-to-invent guard errors before writing |
| User runs `--paste` with no `-o` and no `secrets-store` configured | Med | Low | Hard error with a fix-it message; no secret written anywhere |
| Dropping clap `default_value="."` breaks a script relying on the legacy `.` default | Low | Low | Legacy positional modes retain the `.` fallback; only `--name`/`--paste` require an explicit destination |
| `KEY=VAL` gaining the `--force` guard breaks a script that re-encrypts over an existing file | Low | Med | Deliberate change; documented in Non-Goals + matrix; fix is to pass `--force`. Destination unchanged (still `.`), so only the clobber-without-force case is affected |
| `ArgGroup` can't express runtime input classification fully | Low | Low | Defensive handler check mirroring existing mixed-mode error |

## Open Questions
- [ ] Should `--clear-clipboard` also be honored for the existing `KEY=VAL`/file modes, or only `--paste`? (Proposed: only `--paste`, where a clipboard was actually read.)
- [ ] Should `secrets-store` expansion also support `$ENV` interpolation in addition to `~`, consistent with how other manifest paths are resolved? (Confirm against existing path handling in `config.rs`.)
- [ ] Residual verification ambiguity (panel finding #10): a decrypt failure when the resolved identity *does* match the recipient is true corruption but is indistinguishable from a mismatched-identity skip. Current design fails safe (keep + loud skip). Acceptable, or worth a recipient-fingerprint check?

### Resolved by the review panel (2026-06-27)
- **CRITICAL #1** atomic write + verify-before-rename - integrated ("Atomic write + honest verification").
- **CRITICAL #2** `secrets-store` serde rename + tilde expansion - integrated (Data Model, Phase 5).
- **CRITICAL #3** lazy config load, no global reorder, don't move the CLI-debug log - integrated (Output dir resolution, Phase 5).
- **MAJOR #4** scope `secrets-store` *destination* to `--name`/`--paste` only - integrated
  (Alternative 4). Per Scott's follow-up, the underlying blast-radius cause is also fixed:
  `KEY=VAL` is rerouted through `encrypt_named` and gains the atomic write + `--force` guard, so
  it can no longer silently clobber (matrix + Phase 5).
- **MAJOR #5** `eprintln!` for safety-downgrade messages - integrated (Logging).
- **MAJOR #6** `ArgGroup` import + manual exclusivity validation + parse tests - integrated (Phases 1/4/7).
- **MAJOR #7** explicit output-behavior matrix - integrated.
- **MINOR #8/#9** clipboard timeout/size cap + log chosen tool - integrated (Clipboard read strategy).

## References
- `docs/design/2026-06-27-clipboard-encrypt-handoff.md` - the incident-driven feature handoff
- `docs/design/2026-01-24-manifest-age-subcommand.md` - original `--name`/stdin design
- `docs/design/2026-03-08-age-subcommand-restructure.md` - dropped `--name`, added `KEY=VAL`
- `docs/design/2026-03-20-xdg-config-and-repo-discovery.md` - repo discovery internals
- Real secrets store: `~/repos/scottidler/secrets/.secrets/`

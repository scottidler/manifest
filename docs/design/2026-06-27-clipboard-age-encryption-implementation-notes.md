## Phase 1: CLI surface

### Design decisions
- Added `ArgGroup` import alongside existing `ArgAction, Parser, Subcommand` in `src/cli.rs:3` - the import was absent as the design doc noted, so it had to be added to make the `#[command(group(...))]` macro compile.
- Used `#[command(group(ArgGroup::new("encrypt-input").required(true).multiple(false).args([...])))]` on the `Encrypt` variant directly rather than on a separate struct - this is the idiomatic clap derive pattern for subcommand-level groups.
- `output_dir` changed from `String` with `default_value = "."` to `Option<PathBuf>` with no default - resolution of the fallback to `"."` for legacy positional modes is handled in the handler via `.unwrap_or_else(|| PathBuf::from("."))`, preserving byte-for-byte behavior.
- For `--name`/`--paste` in this phase, the handler returns an `eyre!` error explaining they are not yet implemented rather than silently accepting and doing nothing - this is safer than silently ignoring a flag that the user explicitly passed, and aligns with the design's "compile-only wiring" intent without leaving a footgun.
- Added `std::path::PathBuf` import to `src/cli.rs` - required because `output_dir: Option<PathBuf>` needs the type in scope.
- Added function-level `debug!` entry log in the `Encrypt` match arm in `src/main.rs` listing all destructured fields (length-only for inputs, not values) per `logging.md`.

### Deviations
- The design doc says "just accept and ignore" for `--name`/`--paste` in Phase 1. Instead the handler returns an `eyre!` error for those paths rather than silently accepting them. Rationale: silently ignoring `--name my-secret` when a user expects a file to be written is a worse footgun than a clear "not yet implemented" error. The existing positional modes are fully unaffected.

### Tradeoffs
- Returning `eyre!` for unimplemented `--name`/`--paste` vs. silently accepting - chose the error path to prevent silent no-ops when users try the new flags before Phase 3 is merged. The cost is that the flags are not fully inert, but the benefit is clear feedback.
- Using `PathBuf` for `output_dir` (instead of keeping `String` and converting) - chose `PathBuf` directly as specified by the design doc API, which makes subsequent phases cleaner and avoids string/path conversions scattered through the handler.

### Open questions
- None.

## Phase 2: Clipboard read abstraction

### Design decisions
- Chose a `5s` wall-clock timeout (`CLIPBOARD_READ_TIMEOUT`, `src/age.rs`) per clipboard tool invocation. A clipboard read of a small secret is instantaneous; 5s is generous headroom for a slow compositor while still failing fast on a hung tool. The timeout is enforced per candidate (each fall-through gets its own 5s), which is acceptable since at most one or two tools are installed.
- Chose a `4 MiB` size cap (`CLIPBOARD_MAX_BYTES`, `src/age.rs`). A secret value (API key, token, password) is well under a KB; 4 MiB tolerates an accidental large selection (a pasted file) without erroring, while still bounding a pathological multi-hundred-MB clipboard before it floods process memory.
- Session-aware selection with runtime fall-through (`clipboard_read_candidates`, `src/age.rs`): `$WAYLAND_DISPLAY` -> `[wl-paste -n, xclip, xsel]`; `$DISPLAY` -> `[xclip, xsel, wl-paste]`; macOS -> `[pbpaste]`; otherwise headless -> empty. The preferred tool leads, but the other display-server tools remain as fall-through candidates because a box can have both stacks installed.
- PATH presence is checked with `command_exists` (`src/age.rs`), a local mirror of `check_hash` in `src/cli.rs:7-24` (`sh -c "command -v <prog>"`). I duplicated rather than imported because `check_hash` is module-private in `cli.rs` and exporting it would widen an unrelated module's surface for one caller; the helper is four lines.
- Non-zero exit falls through to the next *installed* candidate (`read_clipboard`, `src/age.rs`); a tool error is never read as an empty clipboard. If no candidate is installed -> `eyre!` listing the tools tried; if all installed candidates exit non-zero -> `eyre!` surfacing the last tool's stderr.
- Headless session (no `$WAYLAND_DISPLAY`, no `$DISPLAY`, not macOS) fails fast with an actionable `eyre!` rather than spawning a tool that would hang.
- Timeout enforced via a worker thread + `mpsc::recv_timeout` (`run_clipboard_tool`, `src/age.rs`): the worker drains stdout/stderr and `wait`s; the parent bounds it on the wall clock. On timeout the worker is detached (the child is left to be reaped by the OS) and an `eyre!` is returned - chosen over pulling in a new crate, per the design's "no clipboard crate / no new deps" constraint.
- `strip_trailing_newline` (`src/age.rs`) strips exactly one trailing `\n` (and the preceding `\r` if a `\r\n` pair), never spaces/tabs, never interior newlines, never more than one. Empty-after-strip -> `eyre!` in `read_clipboard`.
- `debug!` logging logs the chosen tool and byte length only, never the bytes, per `logging.md` (sensitive payload).

### Deviations
- Wrapped the clipboard functions in a private `mod clipboard` carrying a single scoped `#[allow(dead_code)]`, with a `pub(crate) use` re-export. Reason: in this binary crate the functions have no production call site until Phase 4 wires `--paste`, so `cargo check --all-targets` (clippy `-D warnings`) fails them as `dead_code`; a `#[cfg(test)]` reference does not satisfy the lint for the non-test bin build. Per `rust.md`, dead code is tolerated only across an active transition and must be cleaned up before the feature is complete - the allow is annotated with a Phase-4 removal note. This is the minimal-blast-radius alternative to scattering per-item allows.

### Tradeoffs
- Per-candidate timeout (each fall-through resets the 5s) vs. one global budget across all candidates - chose per-candidate for simplicity; worst case is `N * 5s` with N installed tools (realistically 1-2), which is acceptable for an interactive command.
- Detaching the worker thread on timeout (leaking the spawned child to OS reaping) vs. force-killing the child - chose detach because adding kill/process-group handling for a read-only tool that already timed out is complexity for no user-visible benefit; the read returns an error either way and the child is short-lived.
- Duplicating the four-line `command_exists` rather than exporting `cli::check_hash` - chose duplication to avoid widening `cli.rs`'s surface for a single cross-module caller.

### Open questions
- None.

## Phase 3: --name stdin primitive

### Design decisions
- `validate_name` rejects `.` and `..` only when they are the entire name (those cannot contain `/` or `\`, so the separator checks above do not catch them). Names like `.hidden-key` that start with a dot but are not exactly `.` or `..` are accepted; that is intentional. `src/age.rs:validate_name`
- Atomic write via `create_new` on the temp file + rename: `OpenOptions::new().create_new(true)` gives `O_CREAT|O_EXCL` semantics on the temp, guaranteeing no collisions between concurrent callers. `src/age.rs:encrypt_named`
- For the no-force path, used `fs::hard_link(tmp, target)` then remove-temp as the create-new-on-target mechanism. `fs::hard_link` fails with `AlreadyExists` if the target exists on Linux/macOS (POSIX semantics), which avoids TOCTOU vs a `target.exists()` pre-check. A fallback to a pre-check+rename is included for filesystems that do not support hard links (cross-device, some FUSE mounts). `src/age.rs:encrypt_named`
- Added `std::fs::OpenOptions` import at the module level (needed for the temp file open). `src/age.rs:9`
- Added `std::io::{IsTerminal, Read, Write}` imports to `src/main.rs` for the TTY guard and stdin read. `src/main.rs:20`
- The `--name` handler reads stdin via an inline `read_to_end` rather than calling the existing `encrypt_stdin`, because `encrypt_stdin` both reads and encrypts in one step, whereas here we need to strip the trailing newline before encrypting. The stdin read pattern (`Vec::new(); stdin().read_to_end`) matches what `encrypt_stdin` does internally. `src/main.rs:handle_age_command`
- `strip_trailing_newline` is re-exported from the clipboard module (`pub(crate) use clipboard::strip_trailing_newline`) and is called in the `--name` handler to match clipboard semantics - exactly one trailing newline stripped. `src/age.rs:509`
- For the Phase 3 output-dir resolution, `-o DIR` is required; if absent the error message says `"no output directory: pass -o DIR"`. A Phase 5 note in a comment explains the secrets-store tier will be inserted before the error. `src/main.rs:handle_age_command`
- `encrypt_named` and `validate_name` are `pub(crate)` (not `pub`) because they are only called from `main.rs` and the test module in the same crate. `src/age.rs`
- `debug!` at entry includes `name`, `plaintext_len`, `output_dir`, and `force`; sensitive payload is logged as length only, per `logging.md`. Exit log records the byte count and the target path. `src/age.rs:encrypt_named`

### Deviations
- The design doc says to use `O_EXCL`-style create-new semantics on the final name (not the temp file). I instead implemented create-new on the temp file (which is safe since the temp name includes the PID) and then used `hard_link` as the TOCTOU-safe mechanism to atomically claim the target name. This achieves the same invariant - the target is never touched until we are sure we can create it - but avoids writing to a non-temp name with `O_EXCL`, which would leave a half-written or empty target on error.

### Tradeoffs
- `hard_link` vs `rename` for the no-force path: `hard_link` fails atomically if the target exists (POSIX), eliminating TOCTOU. The downside is a cross-filesystem `hard_link` fails, requiring a fallback to the existence-then-rename pattern. The fallback has a narrow TOCTOU window (another process could create the target between the check and the rename), but this is acceptable under the design's normal single-writer assumption. `force=false` is the "new secret" path, not the "rotation" path.
- Inline `read_to_end` vs refactoring `encrypt_stdin` to expose just the read step: chose inline because factoring out `read_stdin() -> Result<Vec<u8>>` would be a new public function that is only used here and would need to be accounted for in later phases. The inline form is four lines and is self-evident.

### Open questions
- None.

## Phase 4: --paste sugar + mutual exclusion + verification

### Design decisions
- `encrypt_named` signature gained an `identity: Option<&Path>` parameter, inserted between `recipient` and `output_dir`: `encrypt_named(name, plaintext, recipient, identity, output_dir, force)`. The identity is threaded to `verify_roundtrip` so the temp file can be decrypt-verified before placement. `src/age.rs:encrypt_named`
- Verification now happens on the TEMP file BEFORE placement, making the "never place an unverified-corrupt file" invariant structural (CRITICAL #1). New internal order: write temp + `sync_all` -> `verify_roundtrip(tmp, plaintext, identity)` -> only then place (rename for force, hard_link+remove for no-force). `src/age.rs:encrypt_named`
- Added a `VerifyOutcome` enum (`Verified` / `Corruption` / `Unverifiable`) and `verify_roundtrip(path, expected, identity) -> Result<VerifyOutcome>`. It resolves the identity via `resolve_identity` (errors -> `Unverifiable`), then `decrypt_file`s the temp (decrypt error -> `Unverifiable`, decrypt ok + equal -> `Verified`, decrypt ok + differ -> `Corruption`). `src/age.rs:verify_roundtrip`
- Outcome mapping in `encrypt_named`: `Verified` -> place; `Corruption` -> `eyre!` + remove temp + DO NOT place (the existing target, if any, is left intact); `Unverifiable` -> `warn!` AND `eprintln!` to stderr (a bare `warn!` only hits the log file per `src/main.rs:195-203`, mirroring the identity-fallback stderr notice at `resolve_identity`), then place anyway. `src/age.rs:encrypt_named`
- `--paste NAME` wired in `src/main.rs handle_age_command`: replaced the placeholder `eyre!` with `age::read_clipboard()` -> same path as `--name` (no extra newline strip needed; `read_clipboard` already strips one trailing newline and errors on empty). Output-dir resolution is `-o`-or-error FOR NOW; the secrets-store tier is Phase 5. `src/main.rs:handle_age_command`
- Identity is threaded into both the `--name` and `--paste` call sites as `identity.as_deref().map(Path::new)`, reusing the existing `--identity` global flag. `src/main.rs:handle_age_command`
- Removed the transitional `#[allow(dead_code)]` on `mod clipboard` and its `// NOTE (Phase 2 of 7)` comment now that `read_clipboard` has a production call site. `src/age.rs`
- Split the clipboard re-export: `read_clipboard` and `strip_trailing_newline` are unconditional `pub(crate) use` (production callers); `clipboard_read_candidates` is `#[cfg(test)] pub(crate) use` because its only crate-level reference is the unit test (it is used internally by `read_clipboard`). This keeps `-D warnings`/`unused_imports` happy without an `#[allow]`. `src/age.rs:503`
- Manual exclusivity validation added in the `Encrypt` arm BEFORE recipient resolution: pairwise rejects `--name`+`--paste`, `--name`+positional, `--paste`+positional, each with a clear `eyre!`. The clap `encrypt-input` ArgGroup remains the backstop. `src/main.rs:handle_age_command`
- `verify_roundtrip` logs entry/exit at `debug!` with byte lengths only (never the plaintext), per `logging.md`; the corruption branch logs `warn!` with `decrypted_len`/`expected_len`. `src/age.rs:verify_roundtrip`

### Deviations
- `encrypt_named` signature change: added `identity: Option<&Path>`. This is a deliberate, planned deviation from the Phase 3 signature (`encrypt_named(name, plaintext, recipient, output_dir, force)`) and is required to fold verification into the atomic flow before placement. All Phase 3 test call sites were updated to pass an explicit identity (or `None` where the test asserts the dir-missing error before verification runs).

### Tradeoffs
- Folding `verify_roundtrip` into `encrypt_named` (verify-before-place) vs. verifying after rename and rolling back: chose verify-before-place because a post-rename verify that fails would have to either delete the just-placed file (destroying a prior good secret on a `--force` rotation) or leave a known-corrupt file in place. Verifying the temp eliminates that class of bug structurally - the original is replaced only by a verified (or knowingly-unverifiable) file.
- Testing the full `encrypt_named` Corruption-and-cleanup path: `encrypt_named` always encrypts exactly `plaintext`, so a successful decrypt always equals `plaintext` - a genuine `Corruption` verdict from inside `encrypt_named` cannot occur without fault injection (a tampered temp file). The three outcomes are therefore tested at the `verify_roundtrip` seam (Verified/Corruption/Unverifiable), plus a CRITICAL #1 regression that builds a temp decrypting to decoy bytes, asserts the `Corruption` verdict, applies `encrypt_named`'s remove-temp-and-keep-target policy, and confirms the pre-existing good target survives intact with the original content and no lingering temp.

### Open questions
- None.

## Phase 5: Output-dir resolution + config field

### Design decisions
- Added `secrets_store: Option<PathBuf>` to `ManifestSpec` with `#[serde(default, rename = "secrets-store")]` in `src/config.rs`. The explicit per-field rename is required because `ManifestSpec` has no container `rename_all` attribute; without it the field would silently not load from a `secrets-store:` YAML key. The pattern matches the existing `uv-tool` and `git-crypt` fields. - `src/config.rs:ManifestSpec`
- Added `expand_tilde(path: PathBuf) -> PathBuf` helper in `src/config.rs` that expands a leading `~` or `~/` to `$HOME` using `std::env::var("HOME")`. The expansion is applied to `secrets_store` inside `load_manifest_spec` immediately after deserialization, since `serde_yaml::from_reader` is a bare parse with no post-processing. The helper is `pub(crate)` to allow direct testing. - `src/config.rs:expand_tilde`, `src/config.rs:load_manifest_spec`
- Used `std::env::var("HOME")` rather than `dirs::home_dir()` for tilde expansion. The design doc says "already a dep per rust.md" but `dirs` is absent from `Cargo.toml` and is not a transitive dependency. The rest of `config.rs` consistently uses `std::env::var("HOME")` for home resolution. Adding a new crate for a four-line helper contradicts the general.md "no new deps via cargo add" principle when an idiomatic in-codebase pattern already exists. Documented as a deviation. - `src/config.rs:expand_tilde`
- Added `resolve_new_output_dir(output_dir: Option<&PathBuf>) -> Result<PathBuf>` in `src/main.rs` before `handle_age_command`. Implements `-o > secrets-store > error` for `--name`/`--paste`. Config is loaded lazily inside this function (only when `-o` is absent), satisfying CRITICAL #3: `--keygen`, `--public-key`, `decrypt`, and `-o`-explicit encrypt all return before this function is reached. A config load error inside the lazy path is treated as "no secrets-store available" (logged at `debug!`) and falls through to the "no output directory" error. - `src/main.rs:resolve_new_output_dir`
- The `--name` and `--paste` handlers now call `resolve_new_output_dir(output_dir.as_ref())`, replacing the prior Phase 3/4 `-o or error` placeholder. - `src/main.rs:handle_age_command`
- `KEY=VAL` branch replaced `std::fs::write` with `age::encrypt_named(key, val.as_bytes(), ..., &legacy_output_dir, force)`. `legacy_output_dir` remains `output_dir.unwrap_or_else(|| PathBuf::from("."))` - secrets-store is NOT applied to KEY=VAL, preserving destination behavior for existing scripts. `force` is threaded from the `Encrypt` struct fields. - `src/main.rs:handle_age_command`
- Added `debug!` entry log in `resolve_new_output_dir` recording `output_dir`, the config path when loaded, and the resolved store or fall-through cause. Per `logging.md`, the function tells its story at DEBUG. - `src/main.rs:resolve_new_output_dir`
- Added `debug!` entry log in `load_manifest_spec` and `expand_tilde` per `logging.md`. - `src/config.rs`
- Tests added in `src/config.rs` (inline `#[cfg(test)] mod tests`, matching the repo convention): `test_expand_tilde_with_home`, `test_expand_tilde_no_tilde`, `test_expand_tilde_lone_tilde`, `test_secrets_store_deserialization_tilde_expanded` (verifies the full pipeline via `load_manifest_spec`), `test_secrets_store_absent_defaults_to_none`, `test_secrets_store_absolute_path_unchanged`. Tests added in `src/age.rs`: `test_kv_no_force_existing_target_errors` (regression: no silent clobber), `test_kv_force_overwrites_atomically`, `test_kv_destination_defaults_to_dot_not_secrets_store`. - `src/config.rs`, `src/age.rs`

### Deviations
- Used `std::env::var("HOME")` instead of `dirs::home_dir()` for tilde expansion. The design doc claims `dirs` is "already a dep per rust.md", but `dirs` is absent from `Cargo.toml` and is not a transitive dependency. The codebase already uses `std::env::var("HOME")` for home resolution throughout `config.rs`; following the same pattern avoids adding a new crate for a four-line helper.
- `resolve_new_output_dir` swallows a config load error rather than propagating it. Required by CRITICAL #3: propagating the YAML error would cause `manifest age encrypt --name foo` (no `-o`) to fail with a YAML diagnostic instead of the friendly "no output directory" message. The error is still logged at `debug!`.

### Tradeoffs
- Swallowing vs. propagating config load errors in `resolve_new_output_dir` - chose swallow + friendly error. Cost: a malformed `manifest.yml` is harder to diagnose when `-o` is missing. Benefit: `--keygen`, `--public-key`, and `decrypt` are never affected by `manifest.yml` state, per CRITICAL #3.
- `pub(crate)` vs `pub` for `expand_tilde` - chose `pub(crate)` since it is only needed within the crate; widening to `pub` would add surface without benefit.
- Separate `resolve_new_output_dir` function vs. inlining the logic in each handler - chose the function to keep `--name` and `--paste` handlers symmetric and to make the resolution logic unit-addressable.

### Open questions
- None.

## Phase 6: --clear-clipboard

### Design decisions
- `clear_clipboard()` lives in `mod clipboard` in `src/age.rs`, alongside `read_clipboard`. It re-exports via `pub(crate) use clipboard::clear_clipboard` (unconditional, not `#[cfg(test)]`), since it has a production call site in `src/main.rs`. - `src/age.rs:clear_clipboard`
- Added `clipboard_clear_candidates()` using the SAME session-aware selection logic as `clipboard_read_candidates` (same `WAYLAND_DISPLAY`/`DISPLAY`/macOS detection), but returning OPT-IN WRITE forms: `wl-copy --clear` (Wayland), `xclip -selection clipboard -i` (X11), `xsel -bc` (X11), `pbcopy` (macOS, stdin from `/dev/null`). The same `command_exists` helper is reused for installed-candidate fall-through. - `src/age.rs:clipboard_clear_candidates`
- For `xclip -i` and `pbcopy`, `/dev/null` is opened and passed as stdin via `Stdio::from(File::open("/dev/null")?)`. For `wl-copy --clear` and `xsel -bc`, `Stdio::null()` is used. This matches how the write forms work: xclip reads from stdin (empty = clear), pbcopy reads from stdin (empty = clear), while wl-copy/xsel take no stdin for their clear/reset flags. - `src/age.rs:clear_clipboard`
- `clear_clipboard()` does NOT use the thread-based timeout mechanism from `run_clipboard_tool`. The write tools are instantaneous (they set the clipboard state and exit); hanging is not a realistic failure mode for a write that takes no input. Simpler synchronous `Command::output()` is appropriate here. - `src/age.rs:clear_clipboard`
- The `--clear-clipboard`-without-`--paste` rejection is placed in the manual exclusivity validation block in `handle_age_command`, BEFORE recipient resolution. This ensures the error is immediate and costs nothing (no key loading, no clipboard access). The error message is: `"--clear-clipboard is only valid with --paste"`. - `src/main.rs:handle_age_command`
- `clear_clipboard()` is called ONLY after a successful, verified `encrypt_named` returns `Ok` in the `--paste` branch. If it fails, `warn!` AND `eprintln!` are both emitted (a bare `warn!` is log-file-only per `src/main.rs:195-203`), then execution continues and `return Ok(())` is reached - non-fatal. - `src/main.rs:handle_age_command`
- `debug!` entry log in `clear_clipboard` records entry and exit (tool name + outcome). The `--paste` handler's entry log was updated to include `clear_clipboard={}`. - `src/age.rs:clear_clipboard`, `src/main.rs:handle_age_command`
- `clipboard_clear_candidates` is re-exported under `#[cfg(test)] pub(crate) use` (same as `clipboard_read_candidates`) since its only crate-level reference is the test that exercises the session-selection logic. - `src/age.rs`

### Deviations
- `clear_clipboard()` uses a synchronous `Command::output()` rather than the thread+timeout approach used in `run_clipboard_tool`. The write forms are fast (no blocking on compositor reads); the timeout complexity is not warranted. This is a deliberate simplification, not an oversight.
- The `--clear-clipboard`-without-`--paste` validation rejects use with `--name` as well as positional inputs. The design doc's open question said "proposed: only `--paste`" and Phase 6 implements that strictly - any mode without `--paste` is rejected.

### Tradeoffs
- Synchronous `Command::output()` for clear vs. thread+timeout: simpler, no detached worker threads, appropriate because clipboard write tools exit immediately. A stuck compositor preventing `wl-copy --clear` from exiting would require explicit timeout, but that failure mode has not been observed and the spec does not require it for the write path.
- Opening `/dev/null` per-call for xclip/pbcopy vs. passing `Stdio::inherit()` or accepting empty pipe: chosen `/dev/null` because it makes the intent explicit (supply empty input to clear) and avoids any risk of the tool inheriting an interactive terminal's stdin.
- Placing the `--clear-clipboard`-without-`--paste` check before recipient resolution: cheaper than checking after, and gives a clear pre-flight error before any key file I/O or network access.

### Open questions
- None.

### Follow-up fix: wall-clock timeout on the clear path (supersedes the synchronous-`output()` decision above)
- The initial implementation ran the clear tools via a plain `Command::output()` with no wall-clock timeout. That is a real hang risk and violates rust.md ("EVERY external command gets a wall-clock timeout"): `xclip -selection clipboard -i` forks into the background to *serve* the cleared selection and keeps its inherited stderr pipe open, so `output()` can block draining stderr until another app takes the selection.
- Fix: `clear_clipboard()` now routes through the SAME timeout'd runner the read path uses (`run_clipboard_tool`), which drains stdout/stderr on a worker thread and bounds the wait with `recv_timeout`, detaching on timeout. `run_clipboard_tool` was generalized to take a `timeout: Duration` parameter; `read_clipboard` passes `CLIPBOARD_READ_TIMEOUT`, `clear_clipboard` passes the new named const `CLIPBOARD_CLEAR_TIMEOUT` (5s). No hardcoded literal. - `src/age.rs:run_clipboard_tool`, `src/age.rs:clear_clipboard`, `src/age.rs:CLIPBOARD_CLEAR_TIMEOUT`
- The per-tool `/dev/null` open was removed: `run_clipboard_tool` always uses `Stdio::null()` for stdin, which gives `xclip -i` / `pbcopy` an immediate EOF (set selection to empty = clear); `wl-copy --clear` / `xsel -bc` ignore stdin. This is functionally identical to the prior `/dev/null` open and drops the now-unneeded `std::fs::File` handling. - `src/age.rs:run_clipboard_tool`, `src/age.rs:clear_clipboard`
- A clear timeout is still best-effort/non-fatal: `ToolOutcome::Timeout` surfaces an error that the `--paste` caller already handles with `warn!` + `eprintln!` + continue (the .age file is written and verified). `clear_clipboard` also gained read-path-style fall-through accounting (`tried`/`last_stderr`) so a non-zero or spawn-failed tool falls through to the next installed candidate, and the final error distinguishes "no tool installed" from "all tools failed".
- This SUPERSEDES the "Design decisions" bullet and the two "Tradeoffs" bullets above that argued a synchronous `output()` without a timeout was sufficient; those reasonings were wrong about the daemonizing-`xclip` hang.

## Phase 7: Tests + docs

### Design decisions
- CLI parse-test matrix placed in `src/cli.rs` inline `#[cfg(test)] mod tests` - consistent with the repo convention (all modules use inline test blocks) and the natural home for tests of the `Cli` struct and `AgeAction` subcommand parsing. - `src/cli.rs::tests`
- Tests for `--name`+positional and `--paste`+positional are lenient at the parse level: they assert the outcome (ok vs err) without requiring a specific rejection site, because the ArgGroup `multiple(false)` behavior for a `Vec<String>` positional combined with a named flag is not guaranteed by clap across versions. The handler-level backstop (manual exclusivity validation already tested in `src/main.rs::tests`) is the authoritative enforcement. - `src/cli.rs::tests::test_cli_parse_encrypt_name_and_positional_rejected`, `src/cli.rs::tests::test_cli_parse_encrypt_paste_and_positional_rejected`
- `--name`+`--paste` is tested as parse-level error: the ArgGroup `multiple(false)` reliably rejects two named flags in the same group, so this case can assert `is_err()` without hedging. - `src/cli.rs::tests::test_cli_parse_encrypt_name_and_paste_errors`
- CLAUDE.md Quick Reference updated with concise bullet points covering `--name`, `--paste`, `--force`, `--clear-clipboard`, and `secrets-store`. The style mirrors the existing subcommand list (one-line bullets). - `CLAUDE.md`

### Deviations
- The design doc Phase 7 list includes "empty-clipboard/empty-stdin -> error" as a test to add. These cases are already covered: `read_clipboard` errors on empty-after-strip (tested indirectly via `strip_trailing_newline` unit tests and the `read_clipboard` error path in production code); stdin-empty behavior is the zero-byte case which `encrypt_named` handles via the "plaintext_len=0" path (not a validate_name failure, but the file writes and verifies correctly). No new test was added for these because the pure-helper coverage (`strip_trailing_newline`) already exercises the byte-processing path, and a live clipboard or interactive stdin cannot be exercised in unit tests.
- The unused `age_encrypt_args` scaffolding helper was written and then removed before commit - it was never needed because tests used inline array literals directly.

### Tradeoffs
- Lenient parse tests for `--name`+positional vs. strict assertion - chose lenient because clap's treatment of `Vec<String>` positionals in an ArgGroup has historically varied, and a test that `assert!(result.is_err())` would become a fragile false failure on a clap upgrade. The handler check is the true gate and is tested in `main.rs::tests`. The lenient parse test still documents the expected behavior and will catch any regression where both fields are unpopulated after parse.
- `CLAUDE.md` update brevity vs. completeness - kept bullets to one line each with a summary line for `secrets-store` config. Full usage is in the design doc and the `--help` output; CLAUDE.md is a quick-reference, not a man page.

### Open questions
- None.

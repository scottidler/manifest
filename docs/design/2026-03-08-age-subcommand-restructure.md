# Design Document: Age Subcommand Restructure

**Author:** Scott Idler
**Date:** 2026-03-08
**Status:** Implemented
**Review Passes Completed:** 5/5

## Summary

Restructure `manifest age` from a flat flag-based interface (`-e`, `-d`) to proper subcommands (`encrypt`, `decrypt`) with support for multiple input modes on encrypt and a `--format` option on decrypt to output either `export KEY='val'` (shell eval) or `KEY=val` (env file for systemd/docker).

## Problem Statement

### Background

The current `manifest age` interface was designed in the original implementation (2026-01-24) with `-e`/`-d` flags for encrypt/decrypt. This worked for the initial use case: encrypting single files and decrypting to shell `export` statements.

Two new requirements have emerged:

1. **Encrypt from KEY=VAL pairs** — Currently `-e` only accepts files. Users need to encrypt key-value pairs directly (e.g., `manifest age encrypt API_KEY=sk-xxx`) which creates a properly-named `.age` file without needing to create a temporary file first.

2. **Decrypt to env file format** — Currently `-d` only outputs `export KEY='val'` lines for shell `eval`. Systemd units need `EnvironmentFile`-compatible format (`KEY=val`, no `export` prefix). There is no way to get decrypted key/value data without the shell wrapper.

### Problem

1. `-e` only supports file input — no way to encrypt a KEY=VAL pair directly
2. `-d` always outputs `export` — no way to get bare `KEY=val` for systemd `EnvironmentFile`
3. The flag-based interface (`-e`/`-d` as mutually exclusive flags) doesn't scale well as modes grow

### Goals

- Replace `-e`/`-d` flags with `encrypt`/`decrypt` subcommands
- `encrypt` accepts one or more files OR one or more KEY=VAL pairs
- `decrypt` supports `--format env|export` (default: `export`) for output format
- Future-proof: `--format` can later support `json`, `yaml`
- Preserve backward compatibility for common workflows
- Keep `--keygen`, `--public-key`, `-i`, `-r` as flags on the `age` subcommand

### Non-Goals

- JSON/YAML output formats (future work, but the `--format` flag enables it)
- Changing the filename-to-variable-name convention
- Changing the identity resolution chain
- Re-encrypting or key rotation workflows

## Proposed Solution

### Overview

Replace the current flag-based approach with subcommands:

```
manifest age encrypt FILE...             # encrypt one or more files
manifest age encrypt KEY=VAL...          # encrypt one or more key-value pairs
manifest age decrypt [PATH]              # export KEY='val' (default)
manifest age decrypt [PATH] -f env       # KEY=val (env file format)
manifest age decrypt [PATH] -f export    # export KEY='val' (explicit)
manifest age --keygen                    # stays as flag
manifest age --public-key                # stays as flag
manifest age -i FILE                     # applies to encrypt/decrypt
manifest age -r KEY                      # applies to encrypt
```

### Architecture

```
┌─────────────────────────────────────────────────────────────────────┐
│                        manifest age                                 │
├──────────────────────────┬──────────────────────────────────────────┤
│  encrypt subcommand      │  decrypt subcommand                      │
│                          │                                          │
│  Input detection:        │  Output format (-f|--format):            │
│  • Contains '=' → KV    │  • export (default): export KEY='val'    │
│  • Otherwise   → FILE   │  • env:              KEY=val             │
│                          │  • json (future):    {"KEY": "val"}      │
│  Accepts 1+ args         │  • yaml (future):    KEY: val            │
├──────────────────────────┴──────────────────────────────────────────┤
│  Shared flags:                                                      │
│  -i, --identity FILE    Identity file                               │
│  -r, --recipient KEY    Public key (encrypt only)                   │
│  --keygen               Generate identity                           │
│  --public-key           Show public key                             │
└─────────────────────────────────────────────────────────────────────┘
```

### CLI Design

#### Encrypt Subcommand

Accepts one or more positional arguments. Auto-detects mode by checking if arguments contain `=`.

```bash
# Encrypt from files
manifest age encrypt ~/.config/github/tokens/scottidler
manifest age encrypt file1.txt file2.txt file3.txt

# Encrypt from KEY=VAL pairs
manifest age encrypt GITHUB_PAT=ghp_xxxx
manifest age encrypt GITHUB_PAT=ghp_xxxx CHATGPT_API_KEY=sk-xxxx

# With specific identity/recipient
manifest age encrypt -i ~/.ssh/id_ed25519 GITHUB_PAT=ghp_xxxx
manifest age encrypt -r "age1xxx..." API_KEY=sk-xxxx

# Stdin (use - as filename)
echo "secret" | manifest age encrypt -
```

**File mode behavior (single file):** Encrypt file contents, output ciphertext to stdout (same as current `-e`). Caller redirects to save.

**File mode behavior (multiple files):** For each file, derive the output name from the input filename (`secret.txt` → `secret.age`), encrypt, and write the `.age` file to the current directory. This avoids ambiguous concatenated stdout.

**KEY=VAL mode behavior:** For each `KEY=VAL` argument:
1. Convert KEY to filename: `GITHUB_PAT` → `github-pat.age` (reverse of `filename_to_var`)
2. Encrypt VAL
3. Write to `github-pat.age` in the current directory (or a specified output directory)

**Mode detection (per argument):**
1. If the argument is a path that exists on disk → file mode
2. If the argument does not exist and contains `=` → KEY=VAL mode
3. If the argument does not exist and has no `=` → error (file not found)

Mixed inputs (files + KEY=VAL) in a single invocation are rejected with an error.

#### Decrypt Subcommand

```bash
# Default: shell export format
manifest age decrypt ~/.../.secrets
# Output: export GITHUB_PAT='ghp_xxxx'

# Env file format
manifest age decrypt ~/.../.secrets -f env
# Output: GITHUB_PAT=ghp_xxxx

# Explicit export format (same as default)
manifest age decrypt ~/.../.secrets -f export
# Output: export GITHUB_PAT='ghp_xxxx'

# Pipe to systemd env file
manifest age decrypt ~/.../.secrets -f env > /run/user/$UID/obsidian-borg.env
```

**Shell startup (unchanged):**
```bash
eval "$(manifest age decrypt ~/.../.secrets)"
```

**Systemd ExecStartPre (new capability):**
```ini
ExecStartPre=/bin/sh -c '/usr/bin/manifest age decrypt /home/user/.../.secrets -f env > /run/user/%%U/obsidian-borg.env'
EnvironmentFile=/run/user/%U/obsidian-borg.env
```

### Data Model

**New CLI Structure (cli.rs):**

```rust
#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Encrypt and decrypt secrets using age encryption
    Age {
        /// Identity file for encryption/decryption
        #[arg(short = 'i', long = "identity", value_name = "FILE")]
        identity: Option<String>,

        /// Recipient public key for encryption (alternative to identity)
        #[arg(short = 'r', long = "recipient", value_name = "KEY")]
        recipient: Option<String>,

        /// Generate a new age identity
        #[arg(long = "keygen")]
        keygen: bool,

        /// Show public key from identity
        #[arg(long = "public-key")]
        public_key: bool,

        #[command(subcommand)]
        action: Option<AgeAction>,
    },
}

#[derive(Debug, Subcommand)]
pub enum AgeAction {
    /// Encrypt files or key-value pairs
    Encrypt {
        /// Files or KEY=VAL pairs to encrypt
        #[arg(required = true, num_args = 1..)]
        inputs: Vec<String>,

        /// Output directory for generated .age files (KEY=VAL and multi-file mode)
        #[arg(short = 'o', long = "output-dir", default_value = ".")]
        output_dir: String,
    },

    /// Decrypt .age files and output key-value pairs
    Decrypt {
        /// Path to .age file or directory containing .age files
        #[arg(default_value = ".")]
        path: String,

        /// Output format: export (default) or env
        #[arg(short = 'f', long = "format", default_value = "export")]
        format: DecryptFormat,
    },
}

#[derive(Debug, Clone, clap::ValueEnum)]
pub enum DecryptFormat {
    Export,
    Env,
}
```

### Output Format Details

**`export` format (default):**
```
export GITHUB_PAT='ghp_xxxx'
export CHATGPT_API_KEY='sk-xxxx'
```
- Values are shell-escaped (existing `shell_escape()` logic)
- Single-quoted for simple values, ANSI-C `$'...'` for special chars
- Ready for `eval`

**`env` format:**
```
GITHUB_PAT=ghp_xxxx
CHATGPT_API_KEY=sk-xxxx
```
- No `export` prefix
- No quoting for simple values
- Values containing whitespace, `#`, `"`, `'`, or `\` are double-quoted with escaping per systemd convention: `KEY="value with spaces"`
- Lines starting with `#` or `;` are comments in systemd, so values starting with these are always quoted
- Suitable for systemd `EnvironmentFile`, Docker `--env-file`, etc.
- Newlines within values are not supported in env format (systemd limitation) — multi-line values will have newlines replaced with `\n` literal

### Key Function: `var_to_filename`

New inverse of `filename_to_var` for KEY=VAL encrypt mode:

```rust
/// Convert environment variable name to filename
/// "CHATGPT_API_KEY" -> "chatgpt-api-key.age"
pub fn var_to_filename(var_name: &str) -> String {
    format!("{}.age", var_name.to_lowercase().replace('_', "-"))
}
```

**Round-trip guarantee:** `filename_to_var(var_to_filename("FOO_BAR"))` == `"FOO_BAR"`. The inverse is also true for canonical filenames (lowercase, hyphens). Files originally named with underscores (e.g. `aws_secret_key.age`) would produce a different canonical filename (`aws-secret-key.age`) but the same variable name.

### Implementation Plan

**Phase 1: Restructure CLI**
- Replace `Age` enum variant's `-e`/`-d` flags with `AgeAction` subcommand enum
- Add `DecryptFormat` enum with `Export` and `Env` variants
- Update `handle_age_command` to dispatch on `AgeAction`
- Keep `--keygen`, `--public-key`, `-i`, `-r` as flags on `Age`

**Phase 2: Decrypt --format**
- Add `render_env()` function alongside `render_exports()`
- Or refactor `render_exports()` to accept a format parameter
- Implement env-file-compatible escaping (different from shell escaping)

**Phase 3: Encrypt KEY=VAL**
- Add `var_to_filename()` function
- Detect KEY=VAL vs file arguments (presence of `=`)
- For KEY=VAL: convert name, encrypt value, write `.age` file
- For files: existing behavior (encrypt, output to stdout)

**Phase 4: Encrypt Multiple Inputs**
- Support variadic positional args for both modes
- All files: encrypt each, output each to stdout (separated)
- All KEY=VAL: encrypt each, write each `.age` file
- Mixed: error (don't mix modes in one invocation)

## Alternatives Considered

### Alternative 1: Separate flags `-D` for env format
- **Description:** Add `-D` alongside `-d` for env output
- **Pros:** Minimal change
- **Cons:** Two decrypt flags is confusing, doesn't scale
- **Why not chosen:** Flag proliferation

### Alternative 2: `--no-export` flag
- **Description:** `manifest age -d --no-export` strips export prefix
- **Pros:** Descriptive
- **Cons:** Negative flags are awkward
- **Why not chosen:** Not extensible to json/yaml

### Alternative 3: Detect output destination (stdout vs file)
- **Description:** stdout → export format, file → env format
- **Pros:** Zero configuration
- **Cons:** Magical, surprising behavior
- **Why not chosen:** Violates principle of least surprise

### Alternative 4: Three-level subcommands (`decrypt export`, `decrypt env`)
- **Description:** Format as a sub-subcommand
- **Pros:** Very explicit
- **Cons:** Three levels deep is awkward to type
- **Why not chosen:** `--format` achieves the same with less nesting

## Technical Considerations

### Dependencies

No new dependencies required. Existing `age` and `clap` crates support the changes.

### Performance

No performance impact. The change is purely at the output formatting layer. Decryption performance remains ~200µs per secret.

### Security

- `env` format values are not shell-escaped, but this is intentional — systemd and docker parse their own format
- Values containing sensitive data are handled identically in both formats
- No new attack surface introduced

### Testing Strategy

**Unit Tests:**
- `var_to_filename()` conversion (inverse of `filename_to_var()`)
- `render_env()` output format
- `render_exports()` unchanged behavior
- KEY=VAL argument detection
- Env-file escaping edge cases (whitespace, `#`, quotes)

**Integration Tests:**
- `manifest age encrypt KEY=VAL` → creates correct `.age` file
- `manifest age decrypt -f env` → correct format
- `manifest age decrypt -f export` → matches current behavior
- Round-trip: encrypt KEY=VAL → decrypt → verify

### Backward Compatibility

The move from `-e`/`-d` flags to subcommands is a **breaking change**. Users must update:

```bash
# Before
manifest age -d ~/.../.secrets
manifest age -e FILE

# After
manifest age decrypt ~/.../.secrets
manifest age encrypt FILE
```

**Mitigation:**
- This is a pre-1.0 tool with a small user base (essentially one user)
- Update `~/.shell-exports.d/secrets.env` at the same time
- Document in changelog

## Risks and Mitigations

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| Ambiguous argument (file doesn't exist, no `=`) | Low | Med | Clear error: "file not found and not a KEY=VAL pair" |
| Env format escaping doesn't match systemd expectations | Med | Med | Test against systemd's `EnvironmentFile` parsing rules |
| Breaking change causes confusion | Low | Low | Pre-1.0, single user, update all references at once |

## Edge Cases

### Encrypt: Value containing `=`
```bash
manifest age encrypt API_KEY=sk-ant=xxx=yyy
```
Split on the **first** `=` only. KEY=`API_KEY`, VAL=`sk-ant=xxx=yyy`.

### Encrypt: Empty value
```bash
manifest age encrypt EMPTY_VAR=
```
Valid. Creates `empty-var.age` with empty ciphertext. Decrypts to `EMPTY_VAR=''`.

### Encrypt: Overwriting existing `.age` file
If `github-pat.age` already exists when running `manifest age encrypt GITHUB_PAT=newvalue`, overwrite it. The user is explicitly providing the new value.

### Decrypt: No `.age` files found
Output nothing, exit 0. Not an error — the directory may just be empty.

### Decrypt: Format for values with `=` in them
```
# env format
API_KEY=sk-ant=xxx=yyy
```
This is valid — env file parsers split on the first `=` only.

### Encrypt: Stdin with `-`
`manifest age encrypt -` reads from stdin, outputs ciphertext to stdout. This is file mode (single input). The caller must redirect to save. Cannot be combined with other arguments.

## Resolved Questions

- [x] **Output directory for `encrypt KEY=VAL`:** Current directory by default, with `-o DIR` flag for specifying an output directory
- [x] **Env format quoting rules:** Target systemd's `EnvironmentFile` parsing rules (unquoted by default, double-quote values containing whitespace/`#`/`'`/`"`)
- [x] **Default path for `decrypt`:** `.` (current directory) — keeps the tool generic; systemd units and shell startup both specify explicit paths anyway

## Expected Help Output

```
$ manifest age --help
Encrypt and decrypt secrets using age encryption

Usage: manifest age [OPTIONS] [COMMAND]

Commands:
  encrypt  Encrypt files or key-value pairs
  decrypt  Decrypt .age files and output key-value pairs

Options:
  -i, --identity <FILE>    Identity file for encryption/decryption
  -r, --recipient <KEY>    Recipient public key for encryption
      --keygen             Generate a new age identity
      --public-key         Show public key from identity

$ manifest age encrypt --help
Encrypt files or key-value pairs

Usage: manifest age encrypt <INPUTS>...

Arguments:
  <INPUTS>...  Files or KEY=VAL pairs to encrypt

$ manifest age decrypt --help
Decrypt .age files and output key-value pairs

Usage: manifest age decrypt [OPTIONS] [PATH]

Arguments:
  [PATH]  Path to .age file or directory [default: .]

Options:
  -f, --format <FORMAT>  Output format [default: export] [possible values: export, env]
```

## Before / After

| Operation | Before | After |
|-----------|--------|-------|
| Encrypt file | `manifest age -e FILE` | `manifest age encrypt FILE` |
| Encrypt KV | N/A | `manifest age encrypt KEY=VAL` |
| Decrypt (shell) | `manifest age -d PATH` | `manifest age decrypt PATH` |
| Decrypt (env) | N/A | `manifest age decrypt PATH -f env` |
| Shell startup | `eval "$(manifest age -d ~/...)"` | `eval "$(manifest age decrypt ~/...)"` |
| Systemd env | N/A | `manifest age decrypt PATH -f env > file.env` |

## References

- [Original age subcommand design doc](./2026-01-24-manifest-age-subcommand.md)
- [systemd EnvironmentFile format](https://www.freedesktop.org/software/systemd/man/latest/systemd.exec.html#EnvironmentFile=)
- [Docker --env-file format](https://docs.docker.com/reference/cli/docker/container/run/#env)
- [age encryption specification](https://age-encryption.org/v1)

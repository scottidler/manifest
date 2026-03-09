# Design Document: manifest age Subcommand

**Author:** Scott Idler
**Date:** 2026-01-24
**Status:** Draft
**Review Passes Completed:** 5/5

## Summary

Add an `age` subcommand to manifest for encrypting and decrypting secrets using the age encryption format. Secrets are stored encrypted in `.secrets/` at the repo root and decrypted on-demand during shell startup.

```bash
manifest age -e FILE       # Encrypt a file
manifest age -d [PATH]     # Decrypt .age files, output shell exports
```

## Critical Design Decision: Variable Naming

**How are environment variable names determined?**

**Filename-based convention:**
```
chatgpt-api-key.age       → CHATGPT_API_KEY
youtube-api-key.age       → YOUTUBE_API_KEY
github-pat.age            → GITHUB_PAT
atlassian-api-key.age     → ATLASSIAN_API_KEY
```

**Conversion rules:**
1. Strip `.age` extension
2. Uppercase all characters
3. Replace `-` with `_`

**There is no configuration file mapping.** Variable names are derived purely from filenames.

## Problem Statement

### Background

The manifest tool manages system configuration through YAML manifests, deploying dotfiles from a `HOME/` directory via symlinks. Users store sensitive credentials (API keys, tokens) in plaintext files that get symlinked and loaded via:

```bash
export GITHUB_PAT=$(cat ~/.config/github/tokens/scottidler)
```

### Problem

1. Secrets exist as plaintext on disk
2. Secrets cannot be safely committed to version control
3. No integrated way to manage encrypted secrets

### Goals

- Provide `manifest age` subcommand for encrypting and decrypting secrets
- Store secrets as `.age` encrypted files in `.secrets/` at repo root
- Decrypt and output shell export statements for `eval`
- Achieve <20ms decryption for ~30 secrets (shell startup latency)
- Simple filename-to-variable-name convention

### Non-Goals

- Configuration file for variable name mapping (see Alternatives Considered)
- Passphrase-based encryption (requires interactive input)
- External tool dependency (no `rage` CLI needed)

## Proposed Solution

### Overview

Add `manifest age` subcommand with two modes:
1. **Encrypt (`-e`)**: Encrypt a plaintext file to `.age` format
2. **Decrypt (`-d`)**: Find `.age` files, decrypt, output shell exports

### Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                      manifest CLI                                │
├─────────────────────────────────────────────────────────────────┤
│  manifest --link          Symlink files from HOME/ (unchanged)  │
│  manifest age -e FILE     Encrypt a file (NEW)                  │
│  manifest age -d [PATH]   Decrypt .age files, output exports    │
│  manifest age -i FILE     Specify identity file                 │
└─────────────────────────────────────────────────────────────────┘

Repo Layout:
┌─────────────────┐     ┌─────────────────┐
│  HOME/          │     │  .secrets/      │
│  (dotfiles)     │     │  (encrypted)    │
│                 │     │                 │
│  --link handles │     │  age handles    │
│  symlinks to ~  │     │  encrypt/decrypt│
└─────────────────┘     └─────────────────┘
```

**File Layout Example:**
```
~/...                                # Dotfiles repo (scottidler convention)
├── .secrets/                        # Encrypted secrets (NOT in HOME/)
│   ├── chatgpt-api-key.age          # Encrypted, decrypted by `manifest age -d`
│   ├── github-pat.age
│   └── atlassian-api-key.age
├── HOME/
│   ├── .bashrc                      # → symlinked to ~/.bashrc
│   ├── .config/
│   │   └── starship.toml            # → symlinked to ~/.config/starship.toml
│   └── .shell-exports.d/
│       └── keys.env                 # → symlinked, contains eval call
└── manifest.yml
```

### CLI Design

#### Encryption (`-e|--encrypt`)

Encrypt a plaintext file. Output goes to stdout (redirect to save).

```bash
# Basic encryption (uses default identity, outputs to stdout)
manifest age -e ~/.config/github/tokens/scottidler
# Output: -----BEGIN AGE ENCRYPTED FILE-----
#         YWdlLWVuY3J5cHRpb24ub3JnL3YxCi0+IFgyNTUxOSA...
#         -----END AGE ENCRYPTED FILE-----

# Save to .secrets/ with proper name for GITHUB_PAT env var
manifest age -e ~/.config/github/tokens/scottidler > ~/.../.secrets/github-pat.age

# Encrypt with specific identity
manifest age -e -i ~/.ssh/id_ed25519 ~/.config/api-key > ~/.../.secrets/api-key.age

# Encrypt from stdin
echo "sk-secret-key" | manifest age -e > ~/.../.secrets/openai-api-key.age

# Encrypt with specific recipient public key
manifest age -e -r "age1xxxxxxx..." ~/.config/secret > ~/.../.secrets/secret.age
```

#### Decryption (`-d|--decrypt`)

Find and decrypt `.age` files, output shell export statements.

```bash
# Decrypt all .age files in path (recursive)
manifest age -d ~/...
# Output:
# export CHATGPT_API_KEY='sk-...'
# export GITHUB_PAT='ghp_...'
# export ATLASSIAN_API_KEY='...'

# Decrypt from current directory (default)
cd ~/...
manifest age -d
# Same output

# Decrypt specific directory
manifest age -d ~/.../.secrets/

# Decrypt single file
manifest age -d ~/.../.secrets/github-pat.age
# Output: export GITHUB_PAT='ghp_...'

# Use specific identity file
manifest age -d -i ~/.ssh/id_ed25519 ~/...

# Use in shell rc file (keys.env gets symlinked)
eval "$(manifest age -d ~/...)"
```

#### Identity Management

```bash
# Generate a new age identity (convenience wrapper)
manifest age --keygen
# Output:
# Identity saved to: ~/.config/manifest/identity.txt
# Public key: age1ql3z7hjy54pw3hyww5ayyfg7zqgvc7w3j2elw8zmrj2kg5sfn9aqmcac8p

# Show public key from identity
manifest age --public-key
# Output: age1ql3z7hjy54pw3hyww5ayyfg7zqgvc7w3j2elw8zmrj2kg5sfn9aqmcac8p

# Show public key from specific identity
manifest age --public-key -i ~/.ssh/id_ed25519
# Output: age1xxxxxxx...
```

### Data Model

**New CLI Structure (cli.rs):**

```rust
#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Encrypt and decrypt secrets using age encryption
    Age {
        /// Encrypt a file (output to stdout)
        #[arg(short = 'e', long = "encrypt", value_name = "FILE")]
        encrypt: Option<String>,

        /// Decrypt .age files in PATH, output shell exports
        #[arg(short = 'd', long = "decrypt", value_name = "PATH",
              default_missing_value = ".", num_args = 0..=1)]
        decrypt: Option<String>,

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
    },
}
```

**Identity File Resolution (for decryption):**
1. Explicit `-i/--identity` argument
2. `~/.config/manifest/identity.txt`
3. `~/.ssh/id_ed25519`
4. `~/.ssh/id_rsa`

**Recipient Resolution (for encryption):**
1. Explicit `-r/--recipient` argument (public key string)
2. Public key derived from identity file

### Core Functions

**New module: src/age.rs**

```rust
use age::{Encryptor, Decryptor, Identity, Recipient};
use age::x25519;
use age::armor::{ArmoredReader, ArmoredWriter, Format};
use eyre::Result;
use std::path::{Path, PathBuf};
use std::io::{Read, Write};
use walkdir::WalkDir;

// ============ ENCRYPTION ============

/// Encrypt data to armored age format
pub fn encrypt(
    plaintext: &[u8],
    recipient: &dyn Recipient
) -> Result<Vec<u8>> {
    let encryptor = Encryptor::with_recipients(std::iter::once(recipient))?;
    let mut output = vec![];
    let armor_writer = ArmoredWriter::wrap_output(&mut output, Format::AsciiArmor)?;
    let mut writer = encryptor.wrap_output(armor_writer)?;
    writer.write_all(plaintext)?;
    writer.finish()?.finish()?;
    Ok(output)
}

/// Encrypt a file, return armored ciphertext
pub fn encrypt_file(path: &Path, recipient: &dyn Recipient) -> Result<Vec<u8>> {
    let plaintext = std::fs::read(path)?;
    encrypt(&plaintext, recipient)
}

// ============ DECRYPTION ============

/// Decrypt armored age data
pub fn decrypt(
    ciphertext: &[u8],
    identity: &dyn Identity
) -> Result<Vec<u8>> {
    let armor_reader = ArmoredReader::new(ciphertext);
    let decryptor = Decryptor::new(armor_reader)?;
    let mut output = vec![];
    let mut reader = decryptor.decrypt(std::iter::once(identity))?;
    reader.read_to_end(&mut output)?;
    Ok(output)
}

/// Decrypt a .age file
pub fn decrypt_file(path: &Path, identity: &dyn Identity) -> Result<Vec<u8>> {
    let ciphertext = std::fs::read(path)?;
    decrypt(&ciphertext, identity)
}

// ============ FILE DISCOVERY ============

/// Recursively find all .age files in a directory
pub fn find_age_files(path: &Path) -> Vec<PathBuf> {
    WalkDir::new(path)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map_or(false, |ext| ext == "age"))
        .map(|e| e.path().to_path_buf())
        .collect()
}

// ============ VARIABLE NAMING ============

/// Convert filename to environment variable name
/// "chatgpt-api-key.age" -> "CHATGPT_API_KEY"
pub fn filename_to_var(path: &Path) -> String {
    path.file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_uppercase()
        .replace('-', "_")
}

// ============ SHELL OUTPUT ============

/// Escape value for shell assignment (handle quotes, newlines)
pub fn shell_escape(value: &[u8]) -> String {
    // Handle as UTF-8, escape single quotes
    let s = String::from_utf8_lossy(value);
    s.replace('\'', "'\\''")
}

/// Decrypt all .age files and format as shell exports
pub fn render_exports(path: &Path, identity: &dyn Identity) -> String {
    let files = find_age_files(path);
    let mut output = String::new();

    for file in files {
        let var_name = filename_to_var(&file);
        match decrypt_file(&file, identity) {
            Ok(plaintext) => {
                let escaped = shell_escape(&plaintext);
                output.push_str(&format!("export {}='{}'\n", var_name, escaped));
            }
            Err(e) => {
                // Log critical error
                log::error!("CRITICAL: failed to decrypt {}: {}", file.display(), e);
                output.push_str(&format!("export {}='manifest age command failed'\n", var_name));
            }
        }
    }
    output
}

// ============ IDENTITY MANAGEMENT ============

/// Generate a new x25519 identity
pub fn generate_identity() -> (x25519::Identity, x25519::Recipient) {
    let identity = x25519::Identity::generate();
    let recipient = identity.to_public();
    (identity, recipient)
}

/// Load identity from file (supports age native and SSH keys)
pub fn load_identity(path: &Path) -> Result<Box<dyn Identity>>;

/// Get public key from identity
pub fn get_public_key(identity: &dyn Identity) -> String;
```

### Implementation Plan

**Phase 1: Decryption (MVP)**
- Add `age` crate dependency: `age = { version = "0.11", features = ["armor", "ssh"] }`
- Create `src/age.rs` module
- Add `age` subcommand with `-d/--decrypt`
- Implement `find_age_files()`, `filename_to_var()`, `decrypt_file()`
- Output exports to stdout

**Phase 2: Encryption**
- Add `-e/--encrypt` flag
- Implement `encrypt()`, `encrypt_file()`
- Support stdin input
- Support `-r/--recipient` for public key

**Phase 3: Identity Management**
- Implement `--keygen` to generate new identity
- Implement `--public-key` to show public key
- Auto-detect identity with fallback chain
- Support SSH keys (ed25519, RSA)

**Phase 4: Polish**
- Shell escaping for special characters
- Handle non-UTF8 content
- Comprehensive error messages
- Performance benchmarking
- Documentation

## Examples

### Complete Workflow: Adding a New Secret

```bash
# 1. Generate identity (one time setup)
manifest age --keygen
# Output: Identity saved to: ~/.config/manifest/identity.txt
#         Public key: age1ql3z7hjy54pw3hyww5ayyfg7zqgvc7w3j2elw8zmrj2kg5sfn9aqmcac8p

# 2. Encrypt an existing plaintext secret
manifest age -e ~/.config/github/tokens/scottidler > ~/.../.secrets/github-pat.age

# 3. Verify it decrypts correctly
manifest age -d ~/.../.secrets/github-pat.age
# Output: export GITHUB_PAT='ghp_xxxxxxxxxxxx'

# 4. Remove the plaintext
rm ~/.config/github/tokens/scottidler

# 5. Commit the encrypted secret
cd ~/...
git add .secrets/github-pat.age
git commit -m "Add encrypted github-pat"
```

### Creating a New Secret from Scratch

```bash
# Encrypt a value directly from stdin
echo -n "sk-ant-api03-xxxxx" | manifest age -e > ~/.../.secrets/anthropic-api-key.age

# Verify
manifest age -d ~/.../.secrets/anthropic-api-key.age
# Output: export ANTHROPIC_API_KEY='sk-ant-api03-xxxxx'
```

### Shell Startup Integration

```bash
# In ~/.../HOME/.shell-exports.d/keys.env (gets symlinked to ~/.shell-exports.d/keys.env)
eval "$(manifest age -d ~/...)"
```

### Encrypting for a Different Machine

```bash
# Get the public key from the other machine's identity
ssh other-machine "manifest age --public-key"
# Output: age1other...

# Encrypt for that recipient
manifest age -e -r "age1other..." ./secret.txt > ./secret.age

# Copy to other machine and decrypt there
scp ./secret.age other-machine:~/.../.secrets/
ssh other-machine "manifest age -d ~/.../.secrets/secret.age"
```

### Re-encrypting with a New Key

```bash
# Generate new identity
manifest age --keygen
# Saved to ~/.config/manifest/identity-new.txt

# Decrypt with old key, re-encrypt with new
manifest age -d -i ~/.config/manifest/identity-old.txt ~/.../.secrets/secret.age | \
  manifest age -e -i ~/.config/manifest/identity-new.txt > ~/.../.secrets/secret-new.age

# Replace
mv ~/.../.secrets/secret-new.age ~/.../.secrets/secret.age
```

## Alternatives Considered

### Alternative 1: Configuration File Mapping (NOT CHOSEN)

**Description:** Use manifest.yml to map variable names to files:
```yaml
secrets:
  identity: ~/.ssh/id_ed25519
  CHATGPT_API_KEY: secrets/open-ai/gpt-35.age
  YOUTUBE_API_KEY: secrets/youtube/api-key.age
```

**Pros:**
- Full control over variable names
- Can use any file organization

**Cons:**
- Requires maintaining mapping in two places (file + config)
- More complex implementation
- Breaks convention-over-configuration principle

**Why not chosen:** Filename convention is simpler, self-documenting.

### Alternative 2: External `rage` CLI (NOT CHOSEN)

**Description:** Only implement decryption in manifest, use `rage` for encryption.

**Pros:**
- Less code to maintain
- Leverage existing tool

**Cons:**
- Extra dependency for users
- Inconsistent UX (two different tools)
- Can't customize encryption workflow

**Why not chosen:** Better UX to have both in one tool. The `age` crate supports both.

### Alternative 3: Flag instead of Subcommand (NOT CHOSEN)

**Description:** Use `manifest --age` flag instead of `manifest age` subcommand.

**Pros:**
- Consistent with other manifest flags

**Cons:**
- Encryption needs its own flags (`-e`, `-r`)
- Gets complicated with nested flags
- Subcommand groups related functionality better

**Why not chosen:** Subcommand provides cleaner organization for encrypt/decrypt modes.

## Technical Considerations

### Dependencies

```toml
[dependencies]
age = { version = "0.11", features = ["armor", "ssh"] }
```

### Performance

**Benchmark results (age-benchmark):**

| Secrets | Total Time | Per Secret |
|--------:|------------|------------|
| 10 | 2.0ms | 200µs |
| 20 | 4.3ms | 215µs |
| 30 | 6.3ms | 210µs |
| 50 | 10.7ms | 214µs |

Target: <20ms for typical secret count. **Achieved: ~6ms for 30 secrets.**

### Security

**Threat Model:**
- Encrypted files safe in version control
- Identity file protected (chmod 600)
- Decrypted values only in shell environment (memory)
- No plaintext written to disk by manifest

**Mitigations:**
- Warn if identity file is world-readable
- Use `secrecy` crate for memory handling
- Support SSH keys (protected by ssh-agent)

### Testing Strategy

**Unit Tests:**
- `filename_to_var()` conversion
- `find_age_files()` directory walking
- `encrypt()` / `decrypt()` round-trip
- Identity loading (age native, SSH ed25519, SSH RSA)
- Shell escaping edge cases

**Integration Tests:**
- End-to-end: `manifest age -e` → `manifest age -d` → verify output
- Test fixtures in `test/.secrets/*.age`

## Edge Cases

### Nested Directories
```
.secrets/
├── github/
│   └── pat.age           → PAT
└── aws/
    └── secret-key.age    → SECRET_KEY
```

**Decision:** Use only the filename, not the path.

If you need `GITHUB_PAT`, name the file `github-pat.age` (flat structure recommended).

### Filename Collisions
```
.secrets/foo/api-key.age   → API_KEY
.secrets/bar/api-key.age   → API_KEY (collision!)
```

**Decision:** Error with clear message listing conflicting files.

### Empty .age File
- Warn, output `export VAR=''`

### Non-.age Files in .secrets/
- Ignore them (only process `*.age` files)

### Decryption Failure

If a `.age` file fails to decrypt (wrong identity, corrupted file):

**Behavior:** Continue, set value to error message, log critical error.

**Output:**
```bash
export GITHUB_PAT='manifest age command failed'
export CHATGPT_API_KEY='sk-ant-...'  # This one worked
```

**Logging:** Write critical error to `~/.local/share/manifest/logs/manifest.log`:
```
2026-01-24T18:45:32 CRITICAL: failed to decrypt .secrets/github-pat.age
  reason: no matching identity found
  tried: ~/.config/manifest/identity.txt, ~/.ssh/id_ed25519
```

**Exit code:** 0 (success) - command completed, errors are logged

### Encryption without Identity

If no identity or recipient specified for encryption:

**Behavior:** Error with clear message.

```
error: no recipient specified for encryption
  hint: use -i to specify identity file, or -r to specify public key
  hint: run `manifest age --keygen` to generate a new identity
```

## Open Questions

- [x] Subcommand vs flag? → **Subcommand (`manifest age`)**
- [x] Include encryption? → **Yes (`-e` flag)**
- [x] Config mapping vs filename convention? → **Filename convention**
- [ ] Should `-d` output raw value for single file (no `export`)?
- [ ] Support encrypting to multiple recipients?

## Migration Guide

**Step 1: Generate identity**
```bash
manifest age --keygen
# Output: Identity saved to: ~/.config/manifest/identity.txt
#         Public key: age1ql3z7hjy54pw3hyww5ayyfg7zqgvc7w3j2elw8zmrj2kg5sfn9aqmcac8p

# IMPORTANT: Never commit identity.txt (it's your private key)
```

**Step 2: Create secrets directory**
```bash
mkdir -p ~/.../.secrets/
```

**Step 3: Encrypt existing secrets**
```bash
# Encrypt each secret (name file for desired env var name)
# github-pat.age → GITHUB_PAT
manifest age -e ~/.config/github/tokens/scottidler > ~/.../.secrets/github-pat.age

# chatgpt-api-key.age → CHATGPT_API_KEY
manifest age -e ~/.config/open-ai/gpt-35 > ~/.../.secrets/chatgpt-api-key.age

# atlassian-api-key.age → ATLASSIAN_API_KEY
manifest age -e ~/.config/atlassian/atlassian-api-key > ~/.../.secrets/atlassian-api-key.age
```

**Step 4: Update shell exports**
```bash
# Edit ~/.../HOME/.shell-exports.d/keys.env
# Replace:
#   export GITHUB_PAT=$(cat ~/.config/github/tokens/scottidler)
#   export CHATGPT_API_KEY=$(cat ~/.config/open-ai/gpt-35)
# With:
eval "$(manifest age -d ~/...)"
```

**Step 5: Remove plaintext secrets**
```bash
rm ~/.config/github/tokens/scottidler
rm ~/.config/open-ai/gpt-35
rm ~/.config/atlassian/atlassian-api-key
```

**Step 6: Commit encrypted secrets**
```bash
cd ~/...
git add .secrets/*.age
git add HOME/.shell-exports.d/keys.env
git commit -m "Add encrypted secrets, update keys.env to use manifest age"
```

**Step 7: Test**
```bash
manifest --link | bash   # Re-symlink (keys.env updated)
source ~/.zshrc          # Reload shell
echo $GITHUB_PAT         # Should show decrypted value
```

## References

- [age encryption specification](https://age-encryption.org/v1)
- [age crate docs](https://docs.rs/age/latest/age/)
- [Benchmark results](../tmp/age-benchmark/)

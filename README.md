# manifest
Rust version of my manifest program that generates bash script to install configurations and software

## Version Reporting

The `manifest` binary supports `--version` and `-v` flags:

```
$ manifest --version
manifest v0.1.0-3-gabcdef
```

- The version is driven by the latest annotated git tag and the output of `git describe`.
- If the current commit is exactly at a tag (e.g., `v0.1.0`), the version will be `manifest v0.1.0`.
- If there are additional commits, it will show something like `manifest v0.1.0-3-gabcdef`.

## `link:` Configuration

The `link:` block supports three sub-keys plus arbitrary source-to-destination symlink entries:

| Key | Type | Purpose |
|-----|------|---------|
| `recursive` | bool | Walk source dirs and create individual file symlinks |
| `dirs` | map | Create a single directory-level symlink per entry |
| _(anything else)_ | string | File-level symlink entry (`src: dst`) |

**Reserved key:** `dirs` is a structured sub-field and cannot be used as a plain symlink source name. A manifest entry like `dirs: $HOME/.dirs` will fail to parse with a Serde type error. Rename the source path if it conflicts.

Example:

```yaml
link:
  recursive: true
  HOME: $HOME
  dirs:
    HOME/.claude/skills: $HOME/.claude/skills
```

## Secrets: directory-driven lanes

The folder a secret lives in decides where it goes. `<root>` is `secrets-store`
from `manifest.yml` if set, else the directory holding `manifest.yml`.

```
<root>/
  .env/    <name>.age   -> $NAME, emitted by `manifest secrets env`
  .file/   <name>.age   -> decrypted to secrets.file[<name>] by `manifest secrets deploy`
```

- `manifest secrets env [-f export|env]` emits every `<root>/.env/*.age`. Adding an
  env secret is one step: encrypt it into `.env/`. No list is read.
  - Non-recursive. Subdirectories and names outside `^[a-z][a-z0-9-]*$` are skipped with a warning.
  - A symlink is followed only if its target stays inside `.env/`. One pointing into `.file/` is skipped.
  - Command-level errors (zero stdout, non-zero exit): missing `.env/`, a lane root that is a
    symlink, the same name in both lanes, or a leftover `secrets.env:` list.
- `manifest secrets deploy [--dry-run]` resolves each `secrets.file` name to `<root>/.file/<name>.age`:

  ```yaml
  secrets:
    file:
      work-signing-key: ~/.ssh/identities/work/signing
  ```

- A `.age` directly in `<root>/` or under a leftover `<root>/.secrets/` is a stray: never
  decrypted, warned about, never fatal.
- Warnings go to the log, plus stderr when it is a TTY or `JOURNAL_STREAM` is set (systemd).
- `manifest age decrypt <dir>` refuses a directory holding a `.file/` beside a `.env/`.
  `manifest age decrypt <one-file>` still decrypts any single path.
- `manifest age encrypt --name NAME` without `-o` writes to `<secrets-store>/.env/`. A file
  secret passes `-o <root>/.file`.

## Release & Versioning Process

1. **Bump the version in `Cargo.toml`** to the new release version (e.g., `0.2.0`).
2. **Commit** the change.
3. **Tag** the commit with an annotated tag: `git tag -a v0.2.0 -m "Release v0.2.0"`.
4. **Push** the tag: `git push --tags`.
5. **Build** the binary. The version will be embedded from the tag and `git describe`.
6. **Create a GitHub Release** and upload the binary. The version in the binary will match the release tag.

> If the version in `Cargo.toml` does not match the latest tag, a warning will be printed at build time.

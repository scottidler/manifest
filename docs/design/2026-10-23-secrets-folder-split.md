# Design Document: Secrets Folder Split

**Author:** Scott Idler
**Date:** 2026-10-23
**Status:** Implemented
**Review Passes Completed:** 5/5 (+ review-panel round 1, folded in; layout changed by Scott after the round, see Resolved Decisions)

## Summary

The folder a secret lives in decides where it goes. `keep/.env/*.age` loads into the environment, and `keep/.file/*.age` never does. `.secrets/` goes away. This removes the `secrets.env` allowlist from `manifest.yml`, so adding an env secret is one step (encrypt the file into `.env/`) instead of two. It stays fail-closed: a key file saved into `.file/` cannot reach an environment, whatever the manifest says or forgets.

## Problem Statement

### Background

- `2026-07-17-manifest-driven-secrets.md` retired the blanket `manifest age decrypt .secrets` sweep. It replaced it with an allowlist: `secrets.env` in `manifest.yml` lists every name that `manifest secrets env` emits.
- The allowlist existed to keep file-shaped secrets (the 4 `syncthing-{cert,key}-*` TLS files) out of the shell, borg, and cortex environments.
- That doc considered this exact layout and rejected it (`2026-07-17-manifest-driven-secrets.md:520`): "**Split stores** ... fixes the leak with no fork, but changes the storage layout (breaks 'stays in `.secrets/`')."
- That doc admits folders fix the leak. The only objection was a self-imposed "stays in `.secrets/`" constraint. This doc drops it, and drops `.secrets/` itself.

### Problem

- **Two steps per env secret, and step two gets forgotten.** You encrypt `<name>.age`, then hand-add `<name>` to `secrets.env`. Measured on keep `main` (`ebe3b98`): 71 `.age` files, 62 declared. 4 of the 9 undeclared are the syncthing files, left out on purpose. The other 5 were forgotten:
  - `escote-authelia-wiki-client-secret`, `escote-authelia-wiki-client-secret-digest`, `escote-wiki-db-pass`, `reflex-typesafe-api-key`: all added in `ebe3b98`, the same commit that DID declare `drata-readwrite-api-key`.
  - `hubspot-api-key`: added in `3cf0279`.
- **A forgotten declaration fails silently.** The secret just isn't in the env. Nothing warns you.
- **The list duplicates the filesystem.** `secrets.env` is a copy of `ls .secrets/` minus 4 names. A derived thing that has to be kept in sync by hand drifts, and it has drifted.

### Goals

- Adding an env secret is one action: put `<name>.age` in `.env/`. (Scott, this session: "what would it require to NOT have to do that?")
- Fail-closed holds: no manifest command emits a `.file/` secret into an environment. That covers `secrets env` (shell, borg, cortex, sb units) and the `age decrypt <dir>` sweep. The one exception is an explicit single-file `age decrypt <path>`: naming the file is the intent.
- Directories as the sink signal. (Scott, this session: "why not handle that by different names folders?")
- All 71 existing `.age` files migrate. The 5 forgotten ones land in `.env/` and start loading.

### Non-Goals

- **Not removing `secrets.file`.** A file secret needs a destination path, and the filesystem cannot express that. The `name -> path` map stays.
- **Not declaring the syncthing files in `secrets.file`.** That is still deferred for the per-host reason in the 2026-07-17 doc (no host-scoping in manifest). They move to `.file/` and sit there undeployed, same as today.
- **Not adding a `manifest secrets get <name>` resolver** for scripts that hardcode a ciphertext path. Those callers (7 files, Phase 3) get their paths updated. Parked: revisit if a second layout change ever happens.
- Not touching `age encrypt`/`age decrypt` beyond the one output-dir default in Phase 1, or the identity mechanism.
- No transition machinery (version forks, fallback to the allowlist). Precedent: the 2026-07-17 doc rejected all of it because the migration covers 3 machines the operator controls. Sequencing it beats permanent compat code.

## Proposed Solution

### Overview

```
keep/
  .env/    <name>.age   -> $NAME, emitted by `manifest secrets env`
  .file/   <name>.age   -> decrypted to secrets.file[<name>] by `manifest secrets deploy`
```

- `<root>` is the directory holding the two lanes: `secrets-store` if set, else `<dir-of-manifest.yml>`. `resolve_secrets_dir` (`src/main.rs:577`) becomes `resolve_secrets_root` and drops its `.join(".secrets")`.
- `manifest secrets env` enumerates `<root>/.env/*.age`. No list is read.
- `manifest secrets deploy` resolves each `secrets.file` name to `<root>/.file/<name>.age`.
- `secrets.env` is removed from the schema. If a manifest still has it, that is a loud command-level error with a migration message, never a silent ignore.
- **`secrets-store` changes meaning:** it used to name the `.age` dir itself, and now names the dir that holds `.env/` and `.file/`. keep sets none. A manifest still pointing it at an old `.secrets` dir finds no `.env/` and gets the missing-lane error, so it fails loudly.

### Naming: root `.env/` + `.file/` (Scott's call)

- Scott chose root-level `.env/` and `.file/` over `.secrets/env/` + `.secrets/file/` (2026-10-23).
- The lane dirs are named after the two sinks, `env` and `file`, which are already the CLI's words (`secrets env`, `secrets.file`).
- **`.secrets` was never a convention outside tools respect.** Only two things special-case it: manifest's default store path (`src/main.rs:586`) and `entropy-check:198`. Phase 1 and Phase 3 change both. Checked 2026-10-23: no `.secrets` rule in Claude `settings.json` or any hook.
- **The `.env` dotenv convention does not bite here.** Checked 2026-10-23 in keep:
  - `git check-ignore -v .env/foo.age .file/foo.age` exits 1, so neither path is ignored. keep has no `.gitignore`, and the global `~/.config/git/ignore` has no `.env` rule.
  - Scott's mise config loads no dotenv file.
  - A tool that auto-loads `.env` from cwd (direnv `dotenv`, python-dotenv, docker compose) gets a directory and fails with an is-a-directory error. That fails loudly and leaks nothing.
- **`entropy-check` gets updated.** `dotfiles/HOME/bin/entropy-check:198` walks hidden dirs only if they are named `.secrets`. Phase 3 changes that allow-set to `.env` and `.file`, so the scan keeps covering keep's ciphertext.

### Architecture

#### `secrets env` (the env lane)

- **Lane roots must be plain directories.** If `<root>/.env` or `<root>/.file` is itself a symlink (`symlink_metadata`), that is a command-level error in both lanes. Without this, containment would trust wherever `.env/` resolves, e.g. another root's `.file/`.
- List `<root>/.env/` non-recursively. Subdirectories are skipped with a warning.
- Take each entry ending in `.age`. The stem is the secret name. Because env names are now picked up automatically, a stem must match `^[a-z][a-z0-9-]*$`, which is stricter than `validate_name`. A non-matching stem is skipped with a warning. All 71 current names match (checked 2026-10-23 in `keep/.secrets`: `ls | sed 's/\.age$//' | rg -v '^[a-z][a-z0-9-]*$' | wc -l` = 0).
- Two stems that map to the same `$NAME` after `filename_to_var` cannot happen under that regex. The lane checks anyway and treats it as a command-level error. 0 such pairs today.
- Sort by name, so output is deterministic.
- **Symlink containment.** A symlinked entry (the 3 aliases) is followed only if its canonical target is inside canonical `<root>/.env/`. A symlink pointing into `.file/`, or anywhere else, is skipped with a warning. A dangling symlink fails `canonicalize` and is skipped the same way. This is the fail-closed chokepoint: without it, `.env/x.age -> ../.file/syncthing-key-desk.age` would leak a file secret into every env.
- **Where warnings go.** Skip warnings go to the log (`warn!`) AND to stderr, but stderr only when it is a TTY or `JOURNAL_STREAM` is set. `JOURNAL_STREAM` is set by systemd for units logging to the journal. Reason: the daemons never see the log file, because manifest's log dir is not writable under the units' `ProtectHome=read-only`. Non-interactive shells (scp, rsync, git over ssh) still get no stderr, which is what the existing TTY gate protects.
- Decrypt and emit through the existing `render_secrets_env` (`src/age.rs:1256`). It already takes `names` + `secrets_dir` and owns the per-secret skip, both output formats, and full buffering. The env lane passes the enumerated names and `<root>/.env` as the dir. No change to buffering or the one-write discipline.
- Missing `<root>/.env/` is a command-level error: zero stdout, non-zero exit, the existing TTY-gated banner. A root with no `.env/` means a layout mistake, not "zero secrets". This includes `secrets env` run against a manifest with no secrets at all. Today that emits nothing and exits 0; after this change it errors. No consumer does this: all three pass `-C .../keep/manifest.yml`.
- An empty `.env/` is valid: zero exports, exit 0.
- Hidden or temp files need no special case. `.foo.age` fails `validate_name` (it contains `.`), and the encrypt temp files (`.<name>.age.tmp-*`) don't end in `.age`.

#### `secrets deploy` (the file lane)

- Unchanged except the ciphertext dir: `deploy_secret_files(&entries, &root.join(".file"), ...)` (`src/main.rs:547`).
- A `secrets.file` name with no `<root>/.file/<name>.age` already fails per entry today (report, continue, non-zero exit). That behavior stays.

#### Strays and collisions (both lanes)

- **Stray:** a `.age` directly in `<root>/`, or anywhere under a leftover `<root>/.secrets/`. Never decrypted by either lane. Each lane `warn!`s it, and `secrets env` names it on the TTY-gated stderr line. It does not fail the command: one stray must not drop every secret from every shell.
- **Collision:** the same name in both `.env/` and `.file/`. Command-level error in both lanes. The name would mean two things, and guessing a lane is how a file secret leaks.

#### `age decrypt <dir>` refuses a split store

- `age decrypt <dir>` walks recursively (`find_age_files`, `src/age.rs:85`) with no lane check. Pointed at keep after the split, it would walk into `.file/` (walkdir does not skip hidden dirs) and print the syncthing files as exports. The panel ran this.
- New rule: if the walk finds any directory named `.file` that has a sibling `.env`, `age decrypt <dir>` refuses. That is a command-level error with zero stdout, and the message points at `manifest secrets env`.
- **The check is on canonical paths, not spelling** (implementation audit round 1, 2026-10-23). A *file lane* is a directory named `.file` whose parent also holds a `.env` directory. The sweep canonicalizes `<dir>` and every `.age` file it finds, following symlinks. It refuses if the canonical `<dir>` is, or is inside, a file lane, or if any found file's canonical path is inside one. This closes three spellings the first cut let through, all measured on the Phase 1 binary against a fake store: `cd .file && age decrypt .`, `age decrypt <symlink-to-.file>`, and `age decrypt <root>/.env` holding `x.age -> ../.file/k.age`.
- `age decrypt <file>` on one explicit path is unchanged, including a path inside `.file/`. That is the ad-hoc single-secret primitive, and naming the file is the intent.

#### `age encrypt --name` default output dir

- Today the fallback when `-o` is absent is `secrets-store` (`resolve_new_output_dir`, `src/main.rs:229`). After the split, that writes a stray at the root.
- New fallback: `<secrets-store>/.env`. Env is the common case. A file secret passes `-o <root>/.file` explicitly.
- keep sets no `secrets-store`, so keep's own flow (`-o`) is unaffected. This only fixes the documented path for a manifest that sets one.

### Data Model

```rust
// src/config.rs
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SecretsSpec {
    /// Retired. True when a leftover `secrets.env:` key is present, so it can
    /// be a migration error instead of serde's generic "unknown field `env`".
    #[serde(default, rename = "env", deserialize_with = "key_present", skip_serializing)]
    pub legacy_env: bool,
    #[serde(default)]
    pub file: HashMap<String, String>,
}

/// serde calls this only when the key exists, so presence is the signal;
/// the value (list, empty, or null) is swallowed.
fn key_present<'de, D: Deserializer<'de>>(d: D) -> Result<bool, D::Error> {
    serde::de::IgnoredAny::deserialize(d)?;
    Ok(true)
}
```

- The check keys on whether the key is present, not on its value. An `Option<IgnoredAny>` would miss `env:` and `env: null`: serde_yaml loads both as `None` (panel-verified, serde_yaml `de.rs:1517`).
- After load, if `legacy_env`, error out: `secrets.env is retired: env secrets are every .age in <root>/.env/; move the files and delete the list`.
- `secrets env` and `secrets deploy` both run this check, so either command fails loudly against an unmigrated manifest.

### API Design

The CLI surface is unchanged:

```
manifest secrets env [-f export|env]   # emits every <root>/.env/*.age
manifest secrets deploy [--dry-run]    # secrets.file names resolve under <root>/.file/
manifest age encrypt --name NAME       # default output dir: <secrets-store>/.env
```

- The consumers are `.zshenv:13`, `borg.yml:195,253`, and `cortex.yml:171`. They already run `manifest -C .../keep/manifest.yml secrets env [-f env]`. None of them change.
- Five ripr systemd units carry that command (verified 2026-10-23: `grep -l "secrets env" ~/.config/systemd/user/*.service`):
  - `borg.service`, `cortex.service`: long-running, need a restart.
  - `sb-harvest.service`, `sb-eval.service`, `sb-eval-report.service`: `Type=oneshot`, so each re-runs `secrets env` on its next start.
- No unit needs a reinstall.

### Implementation Plan

#### Phase 1: manifest: directory-driven lanes
**Model:** opus
**Repo:** `scottidler/manifest`
- `SecretsSpec.env` -> retired `legacy_env` presence flag + migration error, both lanes.
- `secrets_env_context` / `secrets_env`: enumerate `<root>/.env/` per Architecture: non-recursive, `.age` only, name regex, `$NAME` duplicate check, sorted, symlink containment, missing dir = command-level error.
- Lane roots that are symlinks = command-level error, both lanes.
- Stray warning and collision error, both lanes. Warnings go to the log, plus stderr when stderr is a TTY or `JOURNAL_STREAM` is set.
- `resolve_secrets_dir` -> `resolve_secrets_root`, no `.secrets` join.
- `secrets_deploy`: ciphertext dir -> `<root>/.file`.
- `age decrypt <dir>`: refuse a walk that contains a `.file/` dir with a sibling `.env/`.
- `resolve_new_output_dir` fallback -> `<secrets-store>/.env`.
- The `declared` count in `secrets_env_banner` becomes the enumerated count. If enumeration itself fails, it stays `None`.
- Tests, positive and negative, each proven to bite by breaking the code:
  - emits exactly the `.env/` set; ignores a subdir and a non-`.age` file
  - alias inside `.env/` emits under its own name
  - symlink `.env/x.age -> ../.file/k.age` is NOT emitted
  - collision -> zero stdout, non-zero exit
  - legacy `secrets.env:` -> migration error, zero stdout, for each of: a list, an empty value, and `null`
  - `.env/` as a symlink to another dir -> command-level error
  - stem `Bad_Name.age` -> skipped, the rest emit
  - `age decrypt <root>` on a split root -> refused, zero stdout; `age decrypt <root>/.file/k.age` still decrypts
  - missing `.env/` -> zero stdout, non-zero exit
  - stray at root and under a leftover `.secrets/` -> not emitted, the other secrets still emit
  - deploy resolves under `.file/`
  - encrypt fallback writes `<secrets-store>/.env/<name>.age`
- Docs: `README.md`, `CLAUDE.md` (also fixes the stale `secrets-store: ~/repos/scottidler/secrets/.secrets` example at `CLAUDE.md:47`, which now names the root). Add a dated superseded note in `2026-07-17-manifest-driven-secrets.md` at the Split stores alternative, pointing here.
- **Success criteria:** `otto ci` green. Every test named above passes, and fails when its guarded branch is removed.
- **Audit round 1 fold-in** (2026-10-23, follow-up commit on top of Phase 1):
  - `age decrypt <dir>` refusal on canonical paths, per Architecture. New tests, each proven to bite: `cd .file && age decrypt .` refused; a dir symlink to `.file` refused; `.env/x.age -> ../.file/k.age` swept via `age decrypt <root>/.env` refused. Zero stdout in each.
  - `resolve_secrets_root`: a bare `-C manifest.yml` has an empty parent; normalize it to `.` so `find_strays` reads the cwd. Test: a stray at the root warns under both `-C manifest.yml` and `-C ./manifest.yml`.
  - `find_strays` no longer swallows a `read_dir` error on the root: it is a `warn!`, so a missed scan is visible.

#### Phase 2: keep: move the files, drop the list
**Model:** sonnet
**Repo:** `scottidler/keep`
- `git mv` the 4 `syncthing-*.age` -> `.file/`.
- `git mv` the other 67 (62 declared + 5 forgotten, aliases included) -> `.env/`. The 3 aliases are relative links to targets that move with them, so they still resolve.
- `.secrets/` is then empty, and git drops it.
- Delete `secrets.env` from `manifest.yml`. Rewrite the `secrets:` comment block and line 1 header for the new layout.
- Update `keep/CLAUDE.md` (Quick Reference, Architecture tree, Conventions).
- **Success criteria:**
  - `test -e .secrets; echo $?` = `1`, and `ls *.age 2>/dev/null | wc -l` = `0`
  - `yq '.secrets | has("env")' manifest.yml` = `false`
  - The commit is renames only: `git show --format= --name-status -M HEAD -- .secrets .env .file | rg -v '^R100' | wc -l` = 0.
  - Exact name sets: `ls .file` = the 4 `syncthing-*.age`, and `ls .env | wc -l` = 67.
  - Aliases resolve inside `.env/`: `find .env -xtype l | wc -l` = 0, and `readlink` for each of the 3 aliases is a same-dir relative target (a bare filename or `./<name>`, no `..`, not absolute).
    - Amended 2026-10-23 during execution: the original "no `/`" wording failed on `claude-cost-anthropic-api-admin-key.age -> ./anthropic-api-admin-key.age`, which is on main since `c37aeed`. Rewriting it would break the renames-only criterion above.

#### Phase 3: callers that hardcode a ciphertext path
**Model:** sonnet
**Repos:** `scottidler/claude`, `scottidler/dotfiles`
- `claude/HOME/.claude/hooks/hindsight-retain.sh:11` -> `keep/.env/escote-hindsight-api-basic.age`
- `claude/HOME/.claude/bin/hindsight-headers:12` -> same
- `claude/HOME/.claude/hooks/intent-guard.sh:367` -> `keep/.env/github-pat-home.age`
- `claude/HOME/.claude/hooks/manifest-scope-guard-test.sh:43` -> `keep/.env/x.age`
- `claude/HOME/.claude/skills/spotify-playlist/SKILL.md:105`, `scripts/spotify.py:122` -> `keep/.env`
- `claude/HOME/repos/.claude/rules/secrets.md:9,21,22,30,48`. The "all" line (`manifest age decrypt .../.secrets`) becomes `manifest -C ~/repos/scottidler/keep/manifest.yml secrets env`, because a sweep of keep is now refused and `.secrets` no longer exists.
- `dotfiles/HOME/.zshenv:22` comment -> `keep/.env/gh-token.age`.
- `dotfiles/HOME/bin/entropy-check:198`: hidden-dir allow-set `.secrets` -> `.env`, `.file`.
- **Success criteria:** `rg --hidden --no-ignore -g '!.git' -F 'keep/.secrets' claude/HOME dotfiles/HOME | wc -l` = 0. `rg -F '.secrets' dotfiles/HOME/bin/entropy-check | wc -l` = 0. `manifest-scope-guard-test.sh` and `intent-guard-test.sh` pass.
- **Audit round 1 fold-in** (2026-10-23). The list above was scoped to claude and dotfiles, so its `rg` passed by construction. A sweep of every `scottidler/*` and `tatari-tv/*` repo found these, added here:
  - `second-brain/bin/harvest-backfill:10,48,52` -> `keep/.env/escote-anthropic-api-key.age`. Runtime: its fallback decrypt dies after the move.
  - `second-brain/config/templates/borg.yml.example:317` and `cortex.yml.example:212` (commented examples) -> `manifest -C ~/repos/scottidler/keep/manifest.yml secrets env -f env`.
  - `slack-dashboard/docker-compose.yml:2`, `slack-dashboard/CLAUDE.md:74` -> `eval "$(manifest -C ~/repos/scottidler/keep/manifest.yml secrets env)"`.
  - `homelab/gatus/.env.example:2` comment -> `keep/.env/escote-hindsight-api-basic.age`.
  - `keep/CLAUDE.md`: the loader line names `.zshenv` (not `.shell-exports.d/secrets.env`); the encrypt example writes into `.env/`. `keep/manifest.yml` comment: `secrets file` -> `secrets deploy`. These ride the keep branch.
  - Left alone on purpose: test fixtures and goldens (`second-brain/**/tests.rs`, `**/golden/*.service`, `manifest/src/config.rs` tilde-expansion tests) and history (`obsidian/notes/`, `docs/`).
  - Widened criterion: `rg -l --hidden -g '!.git' -g '!target' -g '!**/docs/**' 'keep/[.]secrets' ~/repos/scottidler ~/repos/tatari-tv`, minus fixtures and history = 0, run against the branch worktrees.

## Acceptance Criteria

- [ ] No `secrets.env` list exists: `yq '.secrets | has("env")' keep/manifest.yml` = `false`.
  - Observed on main: `true`
- [ ] The old store is gone: `ls keep/.secrets 2>/dev/null | wc -l` = `0`.
  - Observed on main: `71`
- [ ] Every `.env/` file loads: `manifest -C keep/manifest.yml secrets env | rg -c '^export '` = `ls keep/.env/*.age | wc -l` = `67`.
  - Observed on main: `62` exports (no `.env/` dir yet). The `67` side needs Phase 2.
- [ ] No file secret reaches an env:
  - The `cargo test` cases for containment, the symlinked lane root, collision, and the `age decrypt` refusal pass.
  - `manifest -C keep/manifest.yml secrets env > /dev/null; echo $?` = `0`.
  - `manifest -C keep/manifest.yml secrets env | rg '^export SYNCTHING_' | wc -l` = `0`.
  - Observed on main: the test names can't run yet (Phase 1). Exit code `0`. `SYNCTHING_` line count `0`, because the allowlist omits them.
- [ ] No old-layout path survives: the Phase 3 `rg ... | wc -l` command = `0`.
  - Observed on main: `12`

## Resolved Decisions

- **2026-10-23: Layout is root `.env/` + `.file/`; `.secrets/` is removed.** Scott's call, over the author's rec of `.secrets/env/` + `.secrets/file/`. The author's dotenv objection did not hold up when checked (see Naming). The `entropy-check` objection is fixed in Phase 3. The single-root objection is handled by `<root>` = `secrets-store` or the manifest dir.
- **2026-10-23: The 5 forgotten secrets go to `.env/`.** All 5 are API keys, passwords, or OIDC client secrets. Their siblings `escote-authelia-portainer-client-secret` and its `-digest` are already declared env. None is TLS or SSH material.
- **2026-10-23: A stray warns, it doesn't fail.** Failing would empty every shell's env over one misplaced file. A collision does fail, because there the intent is ambiguous, not just misplaced.
- **2026-10-23: `age decrypt <dir>` refuses a split store; single-file decrypt stays open.** The panel found the sweep leaks `.file/`. The other option was narrowing the guarantee to `secrets env` only. Rejected: it leaves a documented command (`rules/secrets.md:22`) that leaks TLS keys. (Panel round 1; author's pick, matching the panel's pick.)
- **2026-10-23: Not specifying case-insensitive filesystem behavior.** All three hosts are Linux. (Panel round 1 pushback, accepted.)
- **2026-10-23: Rollback is one line, not a coordinated plan.** A heavier plan conflicts with the no-transition-machinery non-goal. (Panel round 1 pushback, accepted.)
- **2026-10-23: The no-store behavior change stays.** `secrets env` against a manifest with no store goes from exit 0 to an error. It is disclosed in Architecture, and no consumer does this. (Panel defer, accepted.)
- **2026-10-23: Ship order is binary-first.** Both panel seats agreed. Reason: a new binary against old keep fails LOUDLY (migration error + TTY banner). An old binary against new keep fails SILENTLY (no `env:` list -> zero exports). See Rollout.
- **2026-10-23: Implementation audit round 1 folded in.** Two must-fix and two cheap-win items, all accepted: the spelling-based sweep refusal becomes canonical, the caller inventory widens past claude and dotfiles, a bare `-C manifest.yml` keeps its stray warnings, and keep's docs finish the update. Deferred, with reasons in the synthesis: the untested `$NAME` duplicate branch (unreachable under the regex), and the sandbox-TMPDIR linker tests (fail the same way at baseline). The live checks come after rollout.

## Alternatives Considered

### Nested `.secrets/env/` + `.secrets/file/`
- **Description:** Keep `.secrets/` as the root, with two plain subdirs.
- **Pros:** `secrets-store` keeps its meaning. `entropy-check` needs no change.
- **Why not chosen:** Scott chose root `.env/` + `.file/` (2026-10-23). The nested layout's other claimed advantage, avoiding the dotenv name, did not hold up when checked.

### Deny-list: env by default, list the exceptions
- **Description:** Every `.age` loads except `secrets.file` names and a new `secrets.exclude`.
- **Why not chosen:** It fails open. A new key file leaks into every env until someone remembers to exclude it. That is the exact failure the 2026-07-17 doc fixed.

### Keep the allowlist, have `age encrypt` append to it
- **Description:** `manifest age encrypt --env NAME` encrypts and edits `secrets.env` in one go.
- **Why not chosen:** It keeps a derived list that drifts with any `git mv`, any hand-made `.age`, or any alias. It also makes `age encrypt` rewrite YAML while preserving comments, which is new machinery. The filesystem already is the list.

### Filename suffix (`<name>.env.age` / `<name>.file.age`)
- **Description:** Sink encoded in the name, one flat dir.
- **Why not chosen:** `validate_name` rejects `.` in names, which is load-bearing for path safety. Two signals (filename and position) would also exist for one fact, so they could disagree.

## Technical Considerations

### Dependencies
- None new. `std::fs::read_dir` + `canonicalize` cover enumeration and containment.

### Performance
- One `read_dir` replaces reading a YAML list.
- Decrypts per shell start rise 62 -> 67 (`.zshenv` runs `secrets env` on every shell).

### Security
- **The fail-closed boundary moves from the YAML list to the directory boundary, plus the symlink check.** Both enforce it in code.
- **New env exposure:** the 5 forgotten secrets enter every shell, borg, and cortex env. That is intended. They were meant to be there.
- **Out of scope: TOCTOU and hardlinks.** TOCTOU (check-then-use race) on symlink containment: canonicalize at check time, then decrypt by path. A hardlink from `.env/` to a `.file/` inode passes both containment and the collision check. Both need a local attacker who can write to keep, and that attacker already owns the account. Same threat model as the 2026-07-17 symlinked-parent check. keep has no hardlinks today. Stated so it's on record.
- **"Loud" means interactive shells and systemd only.** In `.zshenv`, `eval "$(manifest ... secrets env)"` returns 0 even when manifest fails. A non-interactive shell gets no banner and just has no secrets. systemd units see the non-zero exit from `ExecStartPre`.

### Testing Strategy
- Unit tests in `#[cfg(test)] mod tests` per manifest's convention, with tempdir stores and a generated identity (existing fixtures in `src/age.rs` tests).
- Each Phase 1 test is proven to bite by removing its guarded branch.
- Live: the acceptance criteria above, run on desk, lappy, and ripr after rollout.

### Rollout Plan

The cross-repo blast radius is manifest -> keep -> claude + dotfiles. The order that ships it with no silent window:

1. **manifest:** merge Phase 1. Install the new binary on desk, lappy, and ripr.
   - **Gate:** `manifest --version` on each host shows the Phase 1 release before step 2 starts.
   - Until keep moves, `secrets env` fails on each host. Interactive shells and systemd see it. Non-interactive shells just get no secrets. Keep this window short: go straight to step 2.
2. **keep + claude + dotfiles:** push the Phase 2 and Phase 3 commits back to back. On each host, `git pull` keep and claude in the same step, then dotfiles.
   - Reason: `hindsight-headers` runs `set -euo pipefail` and exits 1 on a failed decrypt. Once keep moves, it fails HARD on the old path until claude is pulled. Its "fail soft" label was wrong.
   - The other hooks (`hindsight-retain.sh`, `intent-guard.sh`) do fail soft (`2>/dev/null`, `|| token=`).
3. **ripr units:**
   - `systemctl --user reset-failed borg cortex`, then `systemctl --user restart borg cortex`. Reason: borg has `StartLimitBurst=5` / `StartLimitIntervalSec=60` with `Restart=always` / `RestartSec=5`. During the step-1 window it can trip the start limit and refuse a plain restart.
   - Probe: both active, and `rg -c '^[A-Z0-9_]+=' /run/user/1000/<d>.env` = 67 for each.
   - `sb-harvest`, `sb-eval`, `sb-eval-report` are oneshot. Run `systemctl --user reset-failed` on all three, then check `systemctl --user status` after each one's next scheduled run.
4. **Shells:** open a new shell on each host and run the env-count acceptance criterion.

- Already-open shells keep their old env until restarted. Nothing unsets a loaded var.
- **Rollback:** revert the keep commit and `cargo install` manifest at `v0.4.7` (the current tag). Both on each host, same step.

## Risks and Mitigations

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| Old binary + new keep: zero exports, silently | Med | High | Binary-first rollout. Migrated keep has no `env:`, and an old binary reads that as an empty list, so ordering is the guard. |
| A key file dropped into `.env/` by mistake | Low | High | Same exposure the allowlist had (listing it by mistake). No new mitigation: the dir name is the signal. |
| Symlink in `.env/`, or `.env/` itself, pointing into `.file/` | Low | High | Containment check + lane-root symlink error + tests. |
| `age decrypt <keep>` sweeps `.file/` into an env | Med | High | Split-store refusal + test. `rules/secrets.md` "all" line moves to `secrets env` (Phase 3). |
| A missed hardcoded caller breaks after the move | Med | Med | Phase 3 `rg` criterion = 0. Pull keep and claude in the same step (`hindsight-headers` fails hard). |
| A dotenv-aware tool run in keep trips on the `.env/` dir | Low | Low | It fails with an is-a-directory error, loudly, and leaks nothing. None found in Scott's config. |
| ripr units keep stale env, or borg hits its start limit | Med | Med | Rollout step 3: `reset-failed`, restart, probe counts. |

## Open Questions

- None.

## References

- `docs/design/2026-07-17-manifest-driven-secrets.md` (the allowlist design; Split stores alternative at line 520)
- `src/main.rs:229` `resolve_new_output_dir`, `:515` `secrets_deploy`, `:577` `resolve_secrets_dir` (-> `resolve_secrets_root`), `:612` `secrets_env_context`
- `src/age.rs:1256` `render_secrets_env`, `:1498` `deploy_secret_files`
- `src/config.rs:177` `SecretsSpec`
- `dotfiles/HOME/bin/entropy-check:198`

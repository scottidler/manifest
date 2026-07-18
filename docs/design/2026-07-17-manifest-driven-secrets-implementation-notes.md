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

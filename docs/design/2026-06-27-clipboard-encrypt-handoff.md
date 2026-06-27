# Handoff: First-class clipboard-to-.age encryption

**Author:** Scott Idler (dictated to Claude after a painful session)
**Date:** 2026-06-27
**Status:** Proposed
**Type:** Feature handoff / incident-driven request

## TL;DR

Add a one-shot `manifest age` command that takes a user-chosen name and pulls the
secret value **out of the system clipboard**, writing exactly one correctly-named
`.age` file. No KEY=VAL on the command line, no temp files, no shell-history or
process-list exposure, no clobbering the clipboard.

Proposed shape:

```bash
manifest age encrypt --paste DRATA_READONLY_API_KEY -o ~/repos/scottidler/secrets/.secrets
# reads the clipboard, writes drata-readonly-api-key.age, never echoes the value
```

## Why this exists: what actually happened

The task was trivial on paper: "the secret is in my clipboard, put it in a named
`.age` file." It took **four** rounds and produced a wrong file that got committed
in spirit before being caught. The pain, in order:

1. **No native path for the actual use case.** `manifest age encrypt` today accepts
   either a *file* or a `KEY=VAL` pair. There is no "read the value from somewhere
   that is not argv." So the only way to use a clipboard secret was command
   substitution: `manifest age encrypt KEY="$(wl-paste -n)"`. That works but it
   pushes the plaintext secret into the **process argument list** (visible in
   `ps`/`/proc/<pid>/cmdline`) for the life of the call. For a secrets tool, that is
   exactly the wrong default.

2. **The "verify it works" step destroyed the input.** While sanity-checking the
   command in a scratchpad, a `printf 'my-real-clipboard-secret' | wl-copy` was run
   to simulate a clipboard. That **overwrote the real clipboard**, so the subsequent
   "real" encrypt captured the placeholder, not the Drata key. The placeholder
   happened to be 24 chars, and the only verification done was a length check, so
   "round-trip verified" passed on garbage. The user discovered it by decrypting and
   seeing `my-real-clipboard-secret` in their environment.

   Lesson baked into this proposal: a real command must **never touch the clipboard
   write side**, and any built-in verification must compare against the actual
   ciphertext round-trip, not a length heuristic.

3. **Wrong destination.** The file was first written to `~/.config/secrets/`
   (a directory invented mid-task) when the real, long-established store is
   `~/repos/scottidler/secrets/.secrets/` (40+ `.age` files, decrypted at shell
   startup). It then had to be moved. A first-class command does not fix the
   destination problem by itself, but pairing it with a sensible default or a clear
   `-o` story reduces the chance of this.

The net: a tool whose entire job is handling secrets made the single most common
secret-entry workflow (paste a key, name it, store it) a multi-step, error-prone,
clipboard-clobbering footgun. This should be one command.

## History / why the gap exists

The original `manifest age` (2026-01-24) had a stdin-based interface:

```bash
echo -n "my-secret-value" | manifest age encrypt --name my-secret-name
```

The 2026-03-08 "Age Subcommand Restructure" (see
`docs/design/2026-03-08-age-subcommand-restructure.md`) replaced `-e`/`-d` with
`encrypt`/`decrypt` subcommands and introduced `KEY=VAL` mode. In doing so it
**dropped `--name`** and reduced stdin to file-mode only:

> `manifest age encrypt -` reads from stdin, outputs ciphertext to stdout. This is
> file mode (single input). The caller must redirect to save.

So today stdin gives you *raw ciphertext on stdout* (you redirect it yourself and
pick the filename by hand), and `KEY=VAL` gives you a *named file but argv exposure*.
Neither is "value from elsewhere -> correctly-named file." That capability was
present in the original design, lost in the restructure, and is what this handoff
asks to restore in a stronger form.

## Requirements for the first-class command

1. **Named output, value from clipboard.** The user supplies the variable name
   (e.g. `DRATA_READONLY_API_KEY`); the value comes from the clipboard. Output
   filename follows the existing `var_to_filename` convention
   (`DRATA_READONLY_API_KEY` -> `drata-readonly-api-key.age`).
2. **No plaintext in argv.** The secret value must never appear as a process
   argument. This is the whole point versus `KEY="$(wl-paste)"`.
3. **Read-only on the clipboard.** Never write/clear the clipboard as a side effect.
   (Optionally offer an explicit `--clear-clipboard` flag the user opts into.)
4. **Cross-platform clipboard read.** Detect and use, in order: `wl-paste -n`
   (Wayland), `xclip -selection clipboard -o` / `xsel -b` (X11), `pbpaste` (macOS).
   Fail with a clear message if none is available.
5. **Strip the trailing newline.** Clipboard tools commonly append `\n`; it must not
   end up inside the secret. (`wl-paste -n`, or strip a single trailing newline.)
6. **Honest verification.** If the command self-verifies, it must decrypt the file it
   just wrote and compare to the clipboard bytes it read - not a length check.
7. **Sane destination.** Respect `-o` like the rest of `encrypt`. Consider defaulting
   to the repo-discovered secrets store when one is detected, or at minimum refuse to
   silently invent a new directory.
8. **Empty clipboard is an error**, not an empty `.age` file.

## Proposed interface

Primary (flag on the existing `encrypt` subcommand):

```bash
manifest age encrypt --paste DRATA_READONLY_API_KEY [-o DIR]
```

- With `--paste`, the positional is interpreted as a **name only** (not `KEY=VAL`,
  not a file path). The value is read from the clipboard.
- Mutually exclusive with passing `KEY=VAL` or file inputs.

Alternative / complementary, restore a generic stdin-to-named-file path so the
clipboard case is just one composition:

```bash
# generic: name on the command line, value on stdin, never in argv
wl-paste -n | manifest age encrypt --name DRATA_READONLY_API_KEY -o DIR
```

Restoring `--name` (reads value from stdin, writes the named `.age` file) is the
minimal, orthogonal primitive; `--paste` is then sugar over "read clipboard | --name".
Either or both are acceptable; `--paste` is what makes the common case truly one-shot.

## Design questions to resolve

- **`--paste` vs a dedicated `clip`/`paste` subcommand.** A flag keeps it inside
  `encrypt`; a subcommand (`manifest age paste DRATA_READONLY_API_KEY`) is more
  discoverable. Lean flag for consistency with the restructure's design.
- **Default output dir.** Should `manifest` auto-target the discovered secrets repo
  (it already has repo discovery, see `2026-03-20-xdg-config-and-repo-discovery.md`)
  or keep `.` as default and rely on `-o`? Auto-target would have prevented the
  `~/.config/secrets` detour in this incident.
- **Overwrite behavior.** The restructure says KEY=VAL overwrites silently. For a
  clipboard paste that overwrites an existing key, a confirmation or `--force` may be
  warranted given how easy it is to paste the wrong thing.
- **`--clear-clipboard`.** Opt-in flag to wipe the clipboard after a successful write,
  for users who do not want the secret lingering in the buffer.

## Acceptance criteria

- [ ] `manifest age encrypt --paste NAME` reads the clipboard, writes exactly one
      `<name>.age` using the existing naming convention.
- [ ] The secret value never appears in argv (verified via `ps`/`/proc`).
- [ ] The clipboard is not modified by the command.
- [ ] Trailing newline from the clipboard is stripped.
- [ ] Works on Wayland (`wl-paste`) and X11 (`xclip`/`xsel`); clear error if no
      clipboard tool is found.
- [ ] Empty clipboard -> error, no file written.
- [ ] Round-trip test: `--paste` a known value, decrypt, assert byte-equal.
- [ ] `-o DIR` honored; destination behavior documented.

## References

- `docs/design/2026-01-24-manifest-age-subcommand.md` - original `--name`/stdin design
- `docs/design/2026-03-08-age-subcommand-restructure.md` - dropped `--name`, added KEY=VAL
- `docs/design/2026-03-20-xdg-config-and-repo-discovery.md` - repo discovery for default dir
- Real secrets store: `~/repos/scottidler/secrets/.secrets/`

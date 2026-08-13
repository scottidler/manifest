linker() {
  file=$(realpath "$1")
  link="${2/#\~/$HOME}"
  echo "$link -> $file"
  # Guard: refuse to link a source that already contains a self-referential
  # symlink loop, regardless of how it got there (a bug in this script, a
  # manual ln -s, a different tool). `find -L` has built-in loop detection -
  # it errors and stops instead of recursing forever. Cheap for a plain file
  # (nothing to walk); only meaningful for a `dirs:` directory source.
  loop_err=$(find -L "$file" 2>&1 1>/dev/null)
  if [ -n "$loop_err" ]; then
    echo "ERROR: symlink loop found under $file, refusing to link:" >&2
    echo "$loop_err" >&2
    return 1
  fi
  # An existing symlink is the steady state for BOTH file and directory links,
  # so it has to be settled before anything else. `-f` is false for a symlink
  # pointing at a directory, so treating "already linked" as a file test lets a
  # correctly deployed directory link fall through to `ln -s`, which then nests
  # a self-referential link INSIDE the target (repo/.../voice/voice) on every
  # deploy. That loop is what makes recursive walkers re-traverse the tree.
  if [ -L "$link" ]; then
    if [ "$file" = "$(readlink "$link")" ]; then
      echo "[exists] $link"
      return 0
    fi
    echo "[relink] $link -> $file (was $(readlink "$link"))"
    # -n so an existing link to a directory is replaced, not descended into.
    ln -sfn "$file" "$link"
    return 0
  fi
  if [ -f "$link" ]; then
    orig="$link.orig"
    echo "backing up $orig"
    mv "$link" "$orig"
  fi
  # Guard: ln -s src existing_dir silently nests the symlink inside the directory
  # instead of replacing it. Catch this and abort rather than corrupt the tree.
  if [ -d "$link" ]; then
    echo "ERROR: $link is a real directory; remove it before manifest can create a directory symlink here" >&2
    return 1
  fi
  echo "[create] $link -> $file"
  mkdir -p "$(dirname "$link")"
  ln -s "$file" "$link"
}

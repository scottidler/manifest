linker() {
  file=$(realpath "$1")
  link="${2/#\~/$HOME}"
  echo "$link -> $file"
  if [ -f "$link" ] && [ "$file" != "$(readlink "$link")" ]; then
    orig="$link.orig"
    echo "backing up $orig"
    mv "$link" "$orig"
  elif [ ! -f "$link" ] && [ -L "$link" ]; then
    echo "removing broken link $link"
    unlink "$link"
  fi
  # Guard: ln -s src existing_dir silently nests the symlink inside the directory
  # instead of replacing it. Catch this and abort rather than corrupt the tree.
  if [ -d "$link" ] && [ ! -L "$link" ]; then
    echo "ERROR: $link is a real directory; remove it before manifest can create a directory symlink here" >&2
    return 1
  fi
  if [ -f "$link" ]; then
    echo "[exists] $link"
  else
    echo "[create] $link -> $file"
    mkdir -p "$(dirname "$link")"
    ln -s "$file" "$link"
  fi
}

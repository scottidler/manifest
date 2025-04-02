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
  if [ -f "$link" ]; then
    echo "[exists] $link"
  else
    echo "[create] $link -> $file"
    mkdir -p "$(dirname "$link")"
    ln -s "$file" "$link"
  fi
}

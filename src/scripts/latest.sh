latest() {
  PATTERN="$1"
  LATEST="$2"
  NAME="${3:-"$PATTERN"}"
  echo "Fetching latest release from: $LATEST"
  echo "Using pattern: $PATTERN"
  URL="$(curl -sL "$LATEST" | jq -r ".assets[] | select(.name | test(\"$PATTERN\")) | .browser_download_url")"
  if [[ -z "$URL" ]]; then
    echo "No URL found for pattern: $PATTERN"
    exit 1
  fi
  echo "Downloading from URL: $URL"
  FILENAME=$(basename "$URL")
  TMPDIR=$(mktemp -d /tmp/manifest.XXXXXX)
  pushd "$TMPDIR"
  curl -sSL "$URL" -o "$FILENAME"
  echo "Downloaded $FILENAME"
  if [[ "$FILENAME" =~ \.tar\.gz$ ]]; then
    tar xzf "$FILENAME"
  elif [[ "$FILENAME" =~ \.tbz$ ]]; then
    tar xjf "$FILENAME"
  fi
  BINARY=$(find . -type f -name "$NAME" -exec chmod a+x {} + -print)
  if [[ -z "$BINARY" ]]; then
    echo "No binary found named $NAME"
    exit 1
  fi
  mv "$BINARY" ~/bin/
  echo "Moved $BINARY to ~/bin/"
  popd
  rm -rf "$TMPDIR"
  echo "Cleaned up temporary directory $TMPDIR"
}

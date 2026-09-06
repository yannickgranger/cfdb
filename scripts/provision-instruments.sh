#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
CACHE="${CFDB_BINARY_CACHE:-$HOME/.local/share/cfdb/binaries}"
REGISTRY="${CFDB_INSTRUMENT_REGISTRY:-$HOME/.local/share/cfdb/cargo}"

provision() {
  name=$1; bin=$2; url=$3; package=$4; companion=${5:-}
  rev=$(tr -d '[:space:]' < "$name.rev")
  target="$CACHE/$bin-$rev"
  if [ -x "$target" ] && { [ -z "$companion" ] || [ -x "$CACHE/$companion-$rev" ]; }; then
    printf '%s @ %s cached\n' "$name" "${rev:0:12}"
    return
  fi
  home=$(mktemp -d)
  mkdir -p "$REGISTRY"
  ln -sfn "$REGISTRY" "$home/registry"
  CARGO_HOME="$home" cargo install --git "$url" --rev "$rev" --locked --root "$home" "$package"
  mkdir -p "$CACHE"
  for built in $bin $companion; do
    staged=$(mktemp "$CACHE/.$built-staging.XXXXXX")
    cp "$home/bin/$built" "$staged"
    chmod +x "$staged"
    mv -f "$staged" "$CACHE/$built-$rev"
  done
  rm -rf "$home"
  printf '%s @ %s provisioned%s\n' "$name" "${rev:0:12}" "${companion:+ with $companion}"
}

provision cascade cascade https://agency.lab:3000/yg/cascade.git cascade vocab
provision keel keel https://agency.lab:3000/yg/keel.git keel

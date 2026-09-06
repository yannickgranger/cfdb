#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
CACHE="${CFDB_BINARY_CACHE:-$HOME/.local/share/cfdb/binaries}"
REGISTRY="${CFDB_INSTRUMENT_REGISTRY:-$HOME/.local/share/cfdb/cargo}"
BUILD_ROOT="${CFDB_BUILD_ROOT:-$HOME/.local/share/cfdb/build}"

provision() {
  name=$1; bin=$2; url=$3; package=$4; companion=${5:-}
  rev=$(tr -d '[:space:]' < "$name.rev")
  target="$CACHE/$bin-$rev"
  if [ -x "$target" ] && [ -r "$target.sha256" ] &&
    { [ -z "$companion" ] || { [ -x "$CACHE/$companion-$rev" ] && [ -r "$CACHE/$companion-$rev.sha256" ]; }; }; then
    printf '%s @ %s cached\n' "$name" "${rev:0:12}"
    return
  fi
  mkdir -p "$BUILD_ROOT"
  home=$(mktemp -d "$BUILD_ROOT/install.XXXXXX")
  mkdir -p "$REGISTRY"
  ln -sfn "$REGISTRY" "$home/registry"
  CARGO_HOME="$home" cargo install --git "$url" --rev "$rev" --locked --root "$home" "$package"
  mkdir -p "$CACHE"
  for built in $bin $companion; do
    staged=$(mktemp "$CACHE/.$built-staging.XXXXXX")
    cp "$home/bin/$built" "$staged"
    chmod +x "$staged"
    mv -f "$staged" "$CACHE/$built-$rev"
    sha256sum "$CACHE/$built-$rev" | cut -d" " -f1 > "$CACHE/$built-$rev.sha256"
  done
  rm -rf "$home"
  printf '%s @ %s provisioned%s\n' "$name" "${rev:0:12}" "${companion:+ with $companion}"
}

provision cascade cascade https://agency.lab:3000/yg/cascade.git cascade vocab
provision keel keel https://agency.lab:3000/yg/keel.git keel

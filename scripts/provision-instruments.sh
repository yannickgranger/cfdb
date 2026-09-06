#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
CACHE="${CFDB_BINARY_CACHE:-$HOME/.local/share/cfdb/binaries}"
REGISTRY="${CFDB_INSTRUMENT_REGISTRY:-$HOME/.local/share/cfdb/cargo}"
BUILD_ROOT="${CFDB_BUILD_ROOT:-$HOME/.local/share/cfdb/build}"

staged_matches() {
  built=$1; rev=$2
  path="$CACHE/$built-$rev"
  [ -x "$path" ] || return 1
  if [ ! -r "$path.sha256" ]; then
    printf 'restaging: %s @ %s carries no digest at %s — the file name is a claim with nothing to check it against, so the entry is rebuilt rather than trusted\n' \
      "$built" "${rev:0:12}" "$path.sha256" >&2
    return 1
  fi
  recorded=$(tr -d '[:space:]' < "$path.sha256")
  actual=$(sha256sum "$path" | cut -d' ' -f1)
  [ "$recorded" = "$actual" ] && return 0
  printf 'restaging: %s @ %s is not the file provisioning staged — recorded %s, found %s; a file named for a rev answers as that rev until the digest is read, and the rev is what the entry is rebuilt from\n' \
    "$built" "${rev:0:12}" "${recorded:0:16}" "${actual:0:16}" >&2
  return 1
}

provision() {
  name=$1; bin=$2; url=$3; package=$4; companion=${5:-}
  rev=$(tr -d '[:space:]' < "$name.rev")
  target="$CACHE/$bin-$rev"
  if staged_matches "$bin" "$rev" && { [ -z "$companion" ] || staged_matches "$companion" "$rev"; }; then
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
    sha256sum "$staged" | cut -d" " -f1 > "$staged.sha256"
    mv -f "$staged.sha256" "$CACHE/$built-$rev.sha256"
    mv -f "$staged" "$CACHE/$built-$rev"
  done
  rm -rf "$home"
  printf '%s @ %s provisioned%s\n' "$name" "${rev:0:12}" "${companion:+ with $companion}"
}

provision cascade cascade https://agency.lab:3000/yg/cascade.git cascade vocab
provision keel keel https://agency.lab:3000/yg/keel.git keel

CORPUS_CACHE="${CFDB_CORPUS_CACHE:-$HOME/.local/share/cfdb/corpus}"
DOXA_REV=$(tr -d '[:space:]' < doxa.rev)
dir="$CORPUS_CACHE/$DOXA_REV"
if [ -d "$dir/.git" ]; then
  have=$(git -C "$dir" rev-parse HEAD 2>/dev/null || true)
  if [ "$have" = "$DOXA_REV" ]; then
    printf 'corpus @ %s cached (doxa.rev)\n' "${DOXA_REV:0:12}"
  else
    printf 'FATAL: %s is checked out at %s, not the %s doxa.rev claims — a directory named for a rev is a claim, and rev-parse is what checks it; a corpus checkout is a clone a person may have worked in, so it is refused rather than rebuilt — remove it and re-run\n' \
      "$dir" "${have:0:12}" "${DOXA_REV:0:12}" >&2
    exit 1
  fi
else
  mkdir -p "$CORPUS_CACHE"
  staging=$(mktemp -d "$CORPUS_CACHE/.staging.XXXXXX")
  git clone -q https://agency.lab:3000/yg/doxa.git "$staging/doxa" || {
    printf 'FATAL: the corpus could not be cloned for doxa.rev @ %s — unavailable, never a pass (keel-harness §3.1)\n' "${DOXA_REV:0:12}" >&2
    rm -rf "$staging"; exit 1
  }
  git -C "$staging/doxa" checkout -q "$DOXA_REV" || {
    printf 'FATAL: rev %s, which doxa.rev claims, is not in the corpus clone — unreadable at its pin, unavailable (keel-harness §3.1; keel-dialect §3.3)\n' "$DOXA_REV" >&2
    rm -rf "$staging"; exit 1
  }
  reached=$(git -C "$staging/doxa" rev-parse HEAD)
  [ "$reached" = "$DOXA_REV" ] || {
    printf 'FATAL: the checkout answers %s, not the %s asked for — staging that names a rev it did not reach is the defect this step exists to refuse\n' "${reached:0:12}" "${DOXA_REV:0:12}" >&2
    rm -rf "$staging"; exit 1
  }
  mv "$staging/doxa" "$dir"
  rm -rf "$staging"
  printf 'corpus @ %s provisioned (doxa.rev)\n' "${reached:0:12}"
fi

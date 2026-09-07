#!/usr/bin/env bash

set -euo pipefail

CFDB_BIN="${CFDB_BIN:-cfdb}"
if ! command -v "$CFDB_BIN" >/dev/null 2>&1; then
  SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
  CFDB_WS="$(cd "$SCRIPT_DIR/.." && pwd)"
  for build in target/debug/cfdb target/release/cfdb; do
    if [ -x "$CFDB_WS/$build" ]; then
      CFDB_BIN="$CFDB_WS/$build"
      break
    fi
  done
fi
if ! command -v "$CFDB_BIN" >/dev/null 2>&1 && [ ! -x "$CFDB_BIN" ]; then
  echo "determinism-check: cfdb binary not found (tried PATH + sub-workspace target/)" >&2
  echo "  hint: build it first via 'cargo build -p cfdb-cli' from the cfdb repo root" >&2
  exit 2
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CFDB_WS="$(cd "$SCRIPT_DIR/.." && pwd)"
DEFAULT_FIXTURE="$CFDB_WS/spikes/qa5-utc-now"
WORKSPACE="${1:-$DEFAULT_FIXTURE}"
SCRATCH_ROOT="${CFDB_BUILD_ROOT:-$HOME/.local/share/cfdb/build}/determinism"
mkdir -p "$SCRATCH_ROOT"

if [ ! -d "$WORKSPACE" ] || [ ! -f "$WORKSPACE/Cargo.toml" ]; then
  echo "determinism-check: workspace not found or missing Cargo.toml: $WORKSPACE" >&2
  exit 2
fi

DB_A="$(mktemp -d "$SCRATCH_ROOT/db.XXXXXX")"
DB_B="$(mktemp -d "$SCRATCH_ROOT/db.XXXXXX")"
trap 'rm -rf "$DB_A" "$DB_B"' EXIT

KS="determinism-fixture"

"$CFDB_BIN" extract --workspace "$WORKSPACE" --db "$DB_A" --keyspace "$KS" >/dev/null
"$CFDB_BIN" extract --workspace "$WORKSPACE" --db "$DB_B" --keyspace "$KS" >/dev/null

A_SHA="$("$CFDB_BIN" dump --db "$DB_A" --keyspace "$KS" | sha256sum | cut -d' ' -f1)"
B_SHA="$("$CFDB_BIN" dump --db "$DB_B" --keyspace "$KS" | sha256sum | cut -d' ' -f1)"

if [ "$A_SHA" != "$B_SHA" ]; then
  echo "G1 VIOLATION: two consecutive extractions produced different dumps" >&2
  echo "  workspace: $WORKSPACE" >&2
  echo "  run A sha: $A_SHA" >&2
  echo "  run B sha: $B_SHA" >&2
  exit 1
fi

A_ENRICH="$("$CFDB_BIN" enrich-git-history --db "$DB_A" --keyspace "$KS" --workspace "$WORKSPACE")"
B_ENRICH="$("$CFDB_BIN" enrich-git-history --db "$DB_B" --keyspace "$KS" --workspace "$WORKSPACE")"

if [ "$A_ENRICH" != "$B_ENRICH" ]; then
  echo "G1 VIOLATION: two runs of enrich-git-history produced different reports" >&2
  echo "  workspace: $WORKSPACE" >&2
  printf 'run A:\n%s\n' "$A_ENRICH" >&2
  printf 'run B:\n%s\n' "$B_ENRICH" >&2
  exit 1
fi

if [ -f "$CFDB_WS/Cargo.toml" ]; then
  DB_HIR_A="$(mktemp -d "$SCRATCH_ROOT/db.XXXXXX")"
  DB_HIR_B="$(mktemp -d "$SCRATCH_ROOT/db.XXXXXX")"
  trap 'rm -rf "$DB_A" "$DB_B" "$DB_HIR_A" "$DB_HIR_B"' EXIT
  HIR_KS="determinism-hir-fixture"

  if "$CFDB_BIN" extract --workspace "$CFDB_WS" --db "$DB_HIR_A" --keyspace "$HIR_KS" --hir >/dev/null 2>&1; then
    "$CFDB_BIN" extract --workspace "$CFDB_WS" --db "$DB_HIR_B" --keyspace "$HIR_KS" --hir >/dev/null

    A_HIR_SHA="$("$CFDB_BIN" dump --db "$DB_HIR_A" --keyspace "$HIR_KS" | sha256sum | cut -d' ' -f1)"
    B_HIR_SHA="$("$CFDB_BIN" dump --db "$DB_HIR_B" --keyspace "$HIR_KS" | sha256sum | cut -d' ' -f1)"

    if [ "$A_HIR_SHA" != "$B_HIR_SHA" ]; then
      echo "G1 VIOLATION (--hir): two consecutive --hir extractions produced different dumps" >&2
      echo "  workspace: $CFDB_WS" >&2
      echo "  run A sha: $A_HIR_SHA" >&2
      echo "  run B sha: $B_HIR_SHA" >&2
      exit 1
    fi
    echo "G1 OK (--hir): extract=$A_HIR_SHA  ($CFDB_WS)"
  else
    echo "G1 SKIP (--hir): cfdb binary lacks the \`hir\` Cargo feature; build via \`cargo build -p cfdb-cli --features hir\` to enable. Syn-only G1 still enforced."
  fi
fi

echo "G1 OK: extract=$A_SHA  enrich-git-history=deterministic  ($WORKSPACE)"
exit 0

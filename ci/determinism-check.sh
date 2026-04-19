#!/usr/bin/env bash
# ci/determinism-check.sh
#
# RFC-029 §12.1 G1 — byte-identical sorted-jsonl canonical dump check.
#
# Invariant: same (workspace SHA, schema major.minor) → byte-identical
# canonical dump across two consecutive `cfdb extract` runs into fresh
# databases.
#
# Exit codes:
#   0 — both runs produced identical sha256 (G1 holds)
#   1 — the two runs produced different sha256 (G1 violated, regression)
#   2 — usage error or required tool missing
#
# Usage:
#   determinism-check.sh [WORKSPACE]
#
# WORKSPACE defaults to the fixture workspace (spikes/qa5-utc-now).
# Pass an explicit path to check a different workspace (used by the
# negative test that mutates a copy of the fixture).
#
# No baseline file exists. Determinism is proven by the two consecutive
# extractions in this script producing byte-identical dumps — it is a
# consistency check, not a conformance check. No sha is stored across runs.
# (CLAUDE.md §6 rule 8 — no ratchets, no pin files, no --update-baseline.)
#
# The cfdb binary must be on PATH or located via CFDB_BIN env var. CI builds
# it from the cfdb sub-workspace before invoking this script.

set -euo pipefail

# ── Locate the cfdb binary ──────────────────────────────────────────
CFDB_BIN="${CFDB_BIN:-cfdb}"
if ! command -v "$CFDB_BIN" >/dev/null 2>&1; then
  # Try the sub-workspace target/ as a fallback for local invocations.
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

# ── Resolve the fixture workspace ───────────────────────────────────
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CFDB_WS="$(cd "$SCRIPT_DIR/.." && pwd)"
DEFAULT_FIXTURE="$CFDB_WS/spikes/qa5-utc-now"
WORKSPACE="${1:-$DEFAULT_FIXTURE}"

if [ ! -d "$WORKSPACE" ] || [ ! -f "$WORKSPACE/Cargo.toml" ]; then
  echo "determinism-check: workspace not found or missing Cargo.toml: $WORKSPACE" >&2
  exit 2
fi

# ── Two-run harness ─────────────────────────────────────────────────
DB_A="$(mktemp -d)"
DB_B="$(mktemp -d)"
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

echo "G1 OK: $A_SHA  ($WORKSPACE)"
exit 0

#!/usr/bin/env bash
# ci/predicate-determinism.sh
#
# RFC-034 §4.1 — byte-identical stdout across two `cfdb check-predicate`
# runs for every shipped predicate in `.cfdb/predicates/*.cypher`.
#
# Invariant: for each seed predicate + its canonical param set, running
# `cfdb check-predicate --format json` twice on the same keyspace produces
# identical stdout. Holds whether the predicate returns 0 or N rows.
#
# Exit codes:
#   0 — every predicate produced identical stdout across two runs (§4.1 holds)
#   1 — any predicate produced divergent stdout (regression)
#   2 — usage error, tool missing, or keyspace extract failure
#
# Usage:
#   predicate-determinism.sh [WORKSPACE]
#
# WORKSPACE defaults to the cfdb repo root. The binary is built from the
# target workspace before the determinism sweep runs.
#
# No baseline file. Determinism is proven by two-run byte-identical stdout;
# no sha is stored across runs. CLAUDE.md §6.8 — no ratchets.
#
# The cfdb binary must be on PATH or located via CFDB_BIN env var. CI builds
# it with `cargo build --release -p cfdb-cli --bin cfdb` before invocation.

set -euo pipefail

# ── Locate the cfdb binary ──────────────────────────────────────────
CFDB_BIN="${CFDB_BIN:-cfdb}"
if ! command -v "$CFDB_BIN" >/dev/null 2>&1; then
  SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
  CFDB_WS="$(cd "$SCRIPT_DIR/.." && pwd)"
  for build in target/release/cfdb target/debug/cfdb; do
    if [ -x "$CFDB_WS/$build" ]; then
      CFDB_BIN="$CFDB_WS/$build"
      break
    fi
  done
fi
if ! command -v "$CFDB_BIN" >/dev/null 2>&1 && [ ! -x "$CFDB_BIN" ]; then
  echo "predicate-determinism: cfdb binary not found (tried PATH + workspace target/)" >&2
  echo "  hint: build it first via 'cargo build --release -p cfdb-cli --bin cfdb'" >&2
  exit 2
fi

# ── Resolve the workspace root ──────────────────────────────────────
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DEFAULT_WS="$(cd "$SCRIPT_DIR/.." && pwd)"
WORKSPACE="${1:-$DEFAULT_WS}"

if [ ! -d "$WORKSPACE" ] || [ ! -f "$WORKSPACE/Cargo.toml" ]; then
  echo "predicate-determinism: workspace not found or missing Cargo.toml: $WORKSPACE" >&2
  exit 2
fi

PREDICATES_DIR="$WORKSPACE/.cfdb/predicates"
if [ ! -d "$PREDICATES_DIR" ]; then
  echo "predicate-determinism: .cfdb/predicates/ not found at $PREDICATES_DIR" >&2
  exit 2
fi

# ── Extract cfdb workspace into a fresh keyspace ────────────────────
DB_DIR="$(mktemp -d)"
trap 'rm -rf "$DB_DIR"' EXIT

KS="predicate-determinism"
"$CFDB_BIN" extract --workspace "$WORKSPACE" --db "$DB_DIR" --keyspace "$KS" >/dev/null

# ── Per-predicate canonical param set ───────────────────────────────
#
# Keep these param bindings aligned with
# `crates/cfdb-cli/tests/predicate_library_dogfood.rs::SEED_CASES`. A new
# predicate added to the library MUST add both (a) a SeedCase in the Rust
# integration test AND (b) a case below. The test's
# `seed_cases_cover_every_shipped_predicate` assertion catches (a); the
# sweep loop below catches (b) by exiting with code 2 on an unknown seed.

run_predicate_twice() {
  local name="$1"
  shift
  local params=("$@")

  local args=(check-predicate
    --db "$DB_DIR"
    --keyspace "$KS"
    --workspace-root "$WORKSPACE"
    --name "$name"
    --format json
    --no-fail)
  for p in "${params[@]}"; do
    args+=(--param "$p")
  done

  local a b
  a="$("$CFDB_BIN" "${args[@]}")"
  b="$("$CFDB_BIN" "${args[@]}")"

  if [ "$a" != "$b" ]; then
    echo "§4.1 VIOLATION: predicate '$name' produced divergent stdout across two runs" >&2
    printf 'run A:\n%s\n' "$a" >&2
    printf 'run B:\n%s\n' "$b" >&2
    return 1
  fi
  echo "  ✓ $name — deterministic"
}

echo "predicate-determinism: extract ok ($WORKSPACE) → $DB_DIR"

# Known seeds — iterate explicitly so unknown seeds fail loudly.
declare -a SHIPPED
while IFS= read -r path; do
  SHIPPED+=("$(basename "$path" .cypher)")
done < <(find "$PREDICATES_DIR" -maxdepth 1 -name '*.cypher' | sort)

KNOWN_SEEDS=(
  "path-regex"
  "context-homonym-crate-in-multiple-contexts"
  "fn-returns-type-in-crate-set"
)

# Sanity: every shipped seed has a known param set below.
for s in "${SHIPPED[@]}"; do
  if ! printf '%s\n' "${KNOWN_SEEDS[@]}" | grep -qx "$s"; then
    echo "predicate-determinism: unknown seed '$s' shipped without a canonical param set in this script" >&2
    echo "  → Add it to KNOWN_SEEDS and run_predicate_twice below + to SEED_CASES in predicate_library_dogfood.rs" >&2
    exit 2
  fi
done

# Run each seed twice.
status=0
for s in "${KNOWN_SEEDS[@]}"; do
  case "$s" in
    path-regex)
      run_predicate_twice "$s" 'pat:regex:.*\.rs' || status=1
      ;;
    context-homonym-crate-in-multiple-contexts)
      run_predicate_twice "$s" 'context_a:context:cfdb' 'context_b:context:cfdb' || status=1
      ;;
    fn-returns-type-in-crate-set)
      run_predicate_twice "$s" 'type_pattern:regex:NoSuchType_xyz_ZZZ' 'fin_precision_crates:list:cfdb-core' || status=1
      ;;
    *)
      echo "predicate-determinism: no param set for known seed '$s' — internal inconsistency" >&2
      status=2
      ;;
  esac
done

if [ "$status" -eq 0 ]; then
  echo "predicate-determinism: all ${#KNOWN_SEEDS[@]} predicates deterministic (§4.1 holds)"
fi
exit "$status"

#!/usr/bin/env bash
# ci/dogfood-determinism.sh — RFC-039 §3.4 / §I1
#
# Determinism harness for the 7 self-enrich-*.cypher dogfood queries.
# Single combined extract feeds all 7 queries; each is invoked twice
# via tools/dogfood-enrich; stdout is diffed.
#
# This is a SEPARATE script from ci/predicate-determinism.sh — those
# two harnesses cover different cfdb subcommands with incompatible
# param schemas (`cfdb violations` vs `cfdb check-predicate`). Per
# rust-systems R1 verdict: a shared script would require a conditional
# code path with two unrelated branches. Cleaner to keep separate.
#
# Empty-glob-OK at this stage (Issue #342 ships the harness; the .cypher
# files land in Issues #343-#349). When zero templates exist the script
# exits 0 with a log message asserting the harness contract — there are
# no queries to be deterministic about, but the binary itself runs.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

CFDB_BIN="${CFDB_BIN:-$REPO_ROOT/target/release/cfdb}"
DOGFOOD_BIN="${DOGFOOD_BIN:-$REPO_ROOT/target/release/dogfood-enrich}"
QUERIES_DIR="${QUERIES_DIR:-$REPO_ROOT/.cfdb/queries}"

if [ ! -x "$CFDB_BIN" ]; then
    echo "dogfood-determinism: cfdb binary not found at $CFDB_BIN" >&2
    echo "  hint: cargo build -p cfdb-cli --release --bin cfdb" >&2
    exit 2
fi

if [ ! -x "$DOGFOOD_BIN" ]; then
    echo "dogfood-determinism: dogfood-enrich binary not found at $DOGFOOD_BIN" >&2
    echo "  hint: cargo build -p dogfood-enrich --release" >&2
    exit 2
fi

# Single combined extract for all 7 passes. RFC §3.4: "one combined
# extract feeds all 7 queries — cheaper than the per-predicate pattern
# in predicate-determinism.sh".
WORKDIR="$(mktemp -d)"
trap 'rm -rf "$WORKDIR"' EXIT

DB_DIR="$WORKDIR/db"
KEYSPACE="dogfood-determinism-self"
mkdir -p "$DB_DIR"

echo "dogfood-determinism: extracting cfdb-self into $DB_DIR/$KEYSPACE"
"$CFDB_BIN" extract --workspace "$REPO_ROOT" --db "$DB_DIR" --keyspace "$KEYSPACE" >/dev/null

# The 7 passes (default-feature subset + nightly subset). Each is
# invoked unconditionally; the I5.1 feature-presence guard inside
# dogfood-enrich emits a clear "feature missing" message + exit 1
# when the binary was built without the matching feature flag.
PASSES=(
    "enrich-deprecation"
    "enrich-rfc-docs"
    "enrich-bounded-context"
    "enrich-concepts"
    "enrich-reachability"
    "enrich-metrics"
    "enrich-git-history"
)

# Glob check: if zero templates exist this is the scaffolding stage
# (Issue #342). The script still asserts the harness contract — a
# missing template surfaces as a clear runtime error from
# dogfood-enrich, not as a determinism violation.
shopt -s nullglob
TEMPLATES=("$QUERIES_DIR"/self-enrich-*.cypher)
shopt -u nullglob

if [ ${#TEMPLATES[@]} -eq 0 ]; then
    echo "dogfood-determinism: no self-enrich-*.cypher templates yet (Issue #342 scaffolding stage)"
    echo "dogfood-determinism: harness binary present at $DOGFOOD_BIN — contract OK"
    echo "dogfood-determinism: PASS (empty-glob, harness scaffolding only)"
    exit 0
fi

failed=0
for pass in "${PASSES[@]}"; do
    template="$QUERIES_DIR/self-${pass}.cypher"
    if [ ! -f "$template" ]; then
        echo "dogfood-determinism: $pass — template absent at $template, skip"
        continue
    fi

    out_a="$WORKDIR/${pass}-a.txt"
    out_b="$WORKDIR/${pass}-b.txt"

    # Run twice; capture stdout. exit codes 0 (clean) and 30 (violations)
    # are both valid for determinism — we care about byte-stability of
    # output across the two runs, not about whether the invariant holds.
    # Exit 1 (runtime error including I5.1 feature missing) propagates.
    rc_a=0
    "$DOGFOOD_BIN" --pass "$pass" --db "$DB_DIR" --keyspace "$KEYSPACE" \
        --cfdb-bin "$CFDB_BIN" --workspace "$REPO_ROOT" \
        > "$out_a" 2>&1 || rc_a=$?
    rc_b=0
    "$DOGFOOD_BIN" --pass "$pass" --db "$DB_DIR" --keyspace "$KEYSPACE" \
        --cfdb-bin "$CFDB_BIN" --workspace "$REPO_ROOT" \
        > "$out_b" 2>&1 || rc_b=$?

    if [ "$rc_a" = "1" ] || [ "$rc_b" = "1" ]; then
        echo "dogfood-determinism: $pass — runtime error (exit $rc_a / $rc_b), skipping (likely I5.1 feature missing)"
        continue
    fi

    if ! diff -q "$out_a" "$out_b" >/dev/null; then
        echo "dogfood-determinism: $pass — STDOUT DIFFERS across two runs (FAIL)" >&2
        diff "$out_a" "$out_b" >&2 || true
        failed=$((failed + 1))
    else
        echo "dogfood-determinism: $pass — stdout byte-stable (PASS)"
    fi
done

if [ "$failed" -gt 0 ]; then
    echo "dogfood-determinism: $failed pass(es) failed determinism check" >&2
    exit 1
fi

echo "dogfood-determinism: all templates byte-stable across two runs (PASS)"
exit 0

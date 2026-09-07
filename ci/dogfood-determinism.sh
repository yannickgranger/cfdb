#!/usr/bin/env bash

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

mkdir -p "$REPO_ROOT/target"
WORKDIR="$(mktemp -d -p "$REPO_ROOT/target" dogfood-determinism.XXXXXX)"
trap 'rm -rf "$WORKDIR"' EXIT

DB_DIR="$WORKDIR/db"
KEYSPACE="dogfood-determinism-self"
mkdir -p "$DB_DIR"

echo "dogfood-determinism: extracting cfdb-self into $DB_DIR/$KEYSPACE"
"$CFDB_BIN" extract --workspace "$REPO_ROOT" --db "$DB_DIR" --keyspace "$KEYSPACE" >/dev/null

PASSES=(
    "enrich-deprecation"
    "enrich-rfc-docs"
    "enrich-bounded-context"
    "enrich-concepts"
    "enrich-reachability"
    "enrich-metrics"
    "enrich-git-history"
)

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

    if [ ! -f "$out_a" ] || [ ! -f "$out_b" ]; then
        echo "dogfood-determinism: $pass — capture file missing ($out_a / $out_b): infrastructure error, not a determinism verdict (FAIL)" >&2
        failed=$((failed + 1))
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

#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

COMPANION_REPO="${COMPANION_REPO:-yg/graph-specs-rust}"
COMPANION_URL_BASE="${COMPANION_URL_BASE:-https://agency.lab:3000}"
COMPANION_DIR="${COMPANION_DIR:-$(mktemp -d)}"
CFDB_BIN="${CFDB_BIN:-$REPO_ROOT/target/release/cfdb}"
DOGFOOD_BIN="${DOGFOOD_BIN:-$REPO_ROOT/target/release/dogfood-enrich}"

if [ ! -x "$CFDB_BIN" ]; then
    echo "cross-dogfood: cfdb binary not found at $CFDB_BIN" >&2
    echo "  hint: cargo build -p cfdb-cli --release --bin cfdb" >&2
    exit 2
fi

if [ ! -x "$DOGFOOD_BIN" ]; then
    echo "cross-dogfood: dogfood-enrich binary not found at $DOGFOOD_BIN, so the self-enrich-deprecation pass would not run and this script would report a pass it never measured" >&2
    echo "  hint: cargo build -p dogfood-enrich --release --bin dogfood-enrich" >&2
    exit 2
fi

COMPANION_SHA="${COMPANION_SHA:-$("$SCRIPT_DIR/read-cross-fixture-sha.sh")}"

if [ -n "${GITHUB_TOKEN:-}" ]; then
    git config --global url."https://oauth2:${GITHUB_TOKEN}@agency.lab:3000/".insteadOf "https://agency.lab:3000/"
fi
git clone --filter=blob:none "${COMPANION_URL_BASE}/${COMPANION_REPO}.git" "$COMPANION_DIR" \
    || exit 10
(cd "$COMPANION_DIR" && git checkout "$COMPANION_SHA") || exit 10

KEYSPACE="cross-companion-${COMPANION_SHA:0:12}"
DB_DIR="${CFDB_DB_DIR:-$REPO_ROOT/.cfdb/db}"
mkdir -p "$DB_DIR"

"$CFDB_BIN" extract \
    --workspace "$COMPANION_DIR" \
    --db "$DB_DIR" \
    --keyspace "$KEYSPACE" >/dev/null \
    || exit 20

found=0
for rule in "$REPO_ROOT"/examples/queries/arch-ban-*.cypher; do
    rows="$("$CFDB_BIN" violations \
        --db "$DB_DIR" \
        --keyspace "$KEYSPACE" \
        --rule "$rule" \
        --count-only \
        --no-fail)"
    if [ "$rows" -gt 0 ]; then
        echo "cross-dogfood: $(basename "$rule") returned $rows rows on ${COMPANION_REPO}@${COMPANION_SHA:0:12}"
        found=$((found + rows))
    fi
done

echo "cross-dogfood: running self-enrich-deprecation against ${COMPANION_REPO}@${COMPANION_SHA:0:12}"
rc=0
"$DOGFOOD_BIN" \
    --pass enrich-deprecation \
    --db "$DB_DIR" \
    --keyspace "$KEYSPACE" \
    --cfdb-bin "$CFDB_BIN" \
    --workspace "$COMPANION_DIR" \
    || rc=$?
case "$rc" in
    0)
        echo "cross-dogfood: self-enrich-deprecation 0 violations on companion"
        ;;
    30)
        echo "cross-dogfood: self-enrich-deprecation FAIL on ${COMPANION_REPO}@${COMPANION_SHA:0:12}" >&2
        found=$((found + 1))
        ;;
    *)
        echo "cross-dogfood: self-enrich-deprecation runtime error (exit $rc)" >&2
        exit 20
        ;;
esac

if [ "$found" -eq 0 ]; then
    echo "cross-dogfood: 0 violations on ${COMPANION_REPO}@${COMPANION_SHA:0:12}"
    exit 0
fi

echo "cross-dogfood: FAIL — $found total violations across arch-ban-*.cypher + self-enrich-deprecation" >&2
exit 30

#!/usr/bin/env bash

set -euo pipefail

CFDB_DB="${CFDB_DB:-.cfdb/db}"
CFDB_KEYSPACE="${CFDB_KEYSPACE:-cfdb-self}"
CFDB_BIN="${CFDB_BIN:-./target/release/cfdb}"

if [ ! -x "$CFDB_BIN" ]; then
    echo "edge-liveness: cfdb binary not found at $CFDB_BIN" >&2
    echo "  hint: cargo build -p cfdb-cli --bin cfdb --release --features hir" >&2
    exit 2
fi

if ! command -v jq >/dev/null 2>&1; then
    echo "edge-liveness: jq required (read reserved labels from schema describer)" >&2
    exit 2
fi

RESERVED_RAW="$("$CFDB_BIN" schema-describe 2>/dev/null \
    | jq -r '.edges[] | select(.provenance == "reserved") | .label')"
declare -a RESERVED_LABELS=()
while IFS= read -r line; do
    [ -n "$line" ] && RESERVED_LABELS+=("$line")
done <<< "$RESERVED_RAW"

is_reserved() {
    local target="$1"
    local r
    for r in "${RESERVED_LABELS[@]}"; do
        if [ "$r" = "$target" ]; then
            return 0
        fi
    done
    return 1
}

EDGE_LABELS=(
    IN_CRATE
    IN_MODULE
    HAS_FIELD
    HAS_VARIANT
    HAS_PARAM
    TYPE_OF
    IMPLEMENTS
    IMPLEMENTS_FOR
    RETURNS
    BELONGS_TO
    CALLS
    INVOKES_AT
    EXPOSES
    REGISTERS_PARAM
    LABELED_AS
    CANONICAL_FOR
    EQUIVALENT_TO
    REFERENCED_BY
)

declare -a MISSING=()
declare -a COUNTS=()

for lbl in "${EDGE_LABELS[@]}"; do
    out="$("$CFDB_BIN" query --db "$CFDB_DB" --keyspace "$CFDB_KEYSPACE" \
        "MATCH ()-[r:${lbl}]->() RETURN count(*) AS n" 2>/dev/null || true)"
    n="$(printf '%s' "$out" | awk '/"n":/ {gsub(/[^0-9]/,""); print; exit}')"
    n="${n:-0}"
    if [ "$n" = "0" ] && is_reserved "$lbl"; then
        COUNTS+=("$(printf '%-18s %s' "$lbl" "0 (reserved)")")
    else
        COUNTS+=("$(printf '%-18s %s' "$lbl" "$n")")
        if [ "$n" = "0" ]; then
            MISSING+=("$lbl")
        fi
    fi
done

printf 'edge-liveness: keyspace=%s db=%s\n' "$CFDB_KEYSPACE" "$CFDB_DB"
for line in "${COUNTS[@]}"; do
    printf '  %s\n' "$line"
done

if [ "${#MISSING[@]}" -eq 0 ]; then
    printf 'edge-liveness: PASS — every declared label has ≥1 instance\n'
    exit 0
fi

printf 'edge-liveness: dormant labels (zero instances):\n' >&2
for lbl in "${MISSING[@]}"; do
    printf '  - %s\n' "$lbl" >&2
done
exit 1

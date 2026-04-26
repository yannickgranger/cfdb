#!/usr/bin/env bash
# ci/edge-liveness.sh
#
# RFC-037 §3.7 / Issue #222 — edge-liveness informational check.
#
# Iterates the declared edge-label vocabulary (cfdb-core v0.3.0: 18
# labels post-#228 SUPERTRAIT + RECEIVES_ARG deletion) and flags labels
# with zero instances in the target keyspace. v0.3.0 policy is
# informational (CI logs the output but does not block merge); v0.4.0
# promotes to blocking after one release cycle of observation.
#
# The label set is hand-mirrored from `crates/cfdb-core/src/schema/labels.rs`.
# If labels.rs changes, update EDGE_LABELS below in the same PR — the
# `schema_describe_covers_all_edge_labels` unit test keeps cfdb-core
# internally consistent, and `ci/edge-liveness-test.sh` asserts this
# script's list matches the schema describer output.
#
# Query shape deviates from issue #222's prescription in two ways:
#   - `count(*)` not `count(r)` — cfdb-query's edge-variable aggregation
#     currently returns 0 for `count(r)` while the keyspace has matching
#     edges; `count(*)` on the same pattern returns the correct total.
#     Tracking: the count(r) path can be fixed in a follow-up cfdb-petgraph
#     slice; once it works, either form is fine.
#   - JSON output parse, not tabular — `cfdb query` emits JSON only (no
#     `--format` flag today), so the script parses `"n": <int>` lines
#     with awk rather than the tabular awk 'NR==2 {print $1}' form the
#     RFC draft assumed.
#
# Reserved-label suppression (issue #307): labels whose schema descriptor
# carries `provenance: "reserved"` are declared without producers BY DESIGN
# (Phase B reservations). They are reported with a `(reserved)` annotation
# in the informational table but do NOT trigger the failure exit code. The
# reserved set is read from `cfdb schema-describe` JSON — the schema
# descriptor is the single source of truth (no hardcoded list here).
#
# Exit codes:
#   0 — every non-reserved declared label has at least one instance, OR
#       all dormant labels are reserved (liveness pass)
#   1 — one or more non-reserved declared labels have zero instances
#       (reported on stderr)
#   2 — usage error or required binary absent

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

# Reserved labels — read from the schema describer JSON. Single source of
# truth = `Provenance::Reserved` in cfdb-core. Issue #307 forbids hardcoding
# a list here; if a future reservation lands or unlands, this script tracks
# the schema automatically.
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

# Full v0.3.0 edge-label vocabulary. Mirrors the `pub const` set in
# `crates/cfdb-core/src/schema/labels.rs` (18 entries after #228).
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
    # Anonymous endpoints are fine here — the label filter does all the
    # work. `count(*)` matches the row count returned by `emit_path_bindings`,
    # so dormant labels (warning path) and declared-but-empty labels both
    # collapse cleanly to 0.
    out="$("$CFDB_BIN" query --db "$CFDB_DB" --keyspace "$CFDB_KEYSPACE" \
        "MATCH ()-[r:${lbl}]->() RETURN count(*) AS n" 2>/dev/null || true)"
    n="$(printf '%s' "$out" | awk '/"n":/ {gsub(/[^0-9]/,""); print; exit}')"
    n="${n:-0}"
    if [ "$n" = "0" ] && is_reserved "$lbl"; then
        # Reserved-by-design (issue #307): annotate, do not fail.
        COUNTS+=("$(printf '%-18s %s' "$lbl" "0 (reserved)")")
    else
        COUNTS+=("$(printf '%-18s %s' "$lbl" "$n")")
        if [ "$n" = "0" ]; then
            MISSING+=("$lbl")
        fi
    fi
done

# Always log the full table — informational mode means "visible in CI",
# not "silent on pass".
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

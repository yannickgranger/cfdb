#!/usr/bin/env bash
# ci/edge-liveness-test.sh
#
# Unit tests for ci/edge-liveness.sh per RFC-037 §3.7 / Issue #222 Tests:
#   - Unit: script parses on bash 5; iterates expected labels; exits 0
#     when all labels have ≥1 instance; exits 1 when any zero.
#
# Follows the `ci/read-cross-fixture-sha-test.sh` convention — plain
# shell assertions, no test framework. Exits non-zero on any failure.
#
# Dataset strategy: we build a tiny synthetic keyspace JSON with a
# single edge per label for the happy path, and strip one label for
# the sad path. No real `cfdb` binary is touched by the unit tier —
# the script's CFDB_BIN is a shell stub that reads the synthetic
# keyspace and echoes the per-label count as JSON.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
HARNESS="$SCRIPT_DIR/edge-liveness.sh"

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

fail=0
pass=0

assert_exit() {
    local name="$1" want_exit="$2"
    shift 2
    local got_exit=0
    "$@" >"$TMP/stdout" 2>"$TMP/stderr" || got_exit=$?
    if [ "$got_exit" -eq "$want_exit" ]; then
        pass=$((pass + 1))
        echo "PASS: $name (exit $got_exit)"
    else
        fail=$((fail + 1))
        echo "FAIL: $name — expected exit $want_exit, got $got_exit" >&2
        echo "  stdout:" >&2; sed 's/^/    /' "$TMP/stdout" >&2
        echo "  stderr:" >&2; sed 's/^/    /' "$TMP/stderr" >&2
    fi
}

# --- 1. bash -n (parse on bash 5) ---
bash -n "$HARNESS"
pass=$((pass + 1))
echo "PASS: script parses on $(bash --version | head -1)"

# --- 2. label list matches current cfdb-core schema ---
# Extract the EDGE_LABELS bash-array literal and cross-check against
# the cfdb-core labels.rs source. A new edge label added to labels.rs
# without updating this harness is a drift that Issue #222's whole
# point is to detect.
harness_labels="$(awk '/^EDGE_LABELS=\(/,/^\)/' "$HARNESS" \
    | grep -Eo '[A-Z_]+' | grep -v '^EDGE_LABELS$' | sort -u)"
# Mirror from crates/cfdb-core/src/schema/labels.rs — canonical source.
# This list is expected to match exactly; if labels.rs adds a new entry,
# edge-liveness.sh must be updated in the same PR per RFC-037 §3.7.
expected_labels="$(cat <<'EOF' | sort -u
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
EOF
)"
if [ "$harness_labels" = "$expected_labels" ]; then
    pass=$((pass + 1))
    echo "PASS: harness label list matches v0.3.0 schema (18 entries)"
else
    fail=$((fail + 1))
    echo "FAIL: harness label list drifted from v0.3.0 schema" >&2
    diff <(printf '%s\n' "$harness_labels") <(printf '%s\n' "$expected_labels") >&2 || true
fi

# Dispatch helper for the cfdb stubs — schema-describe returns a schema-shaped
# JSON, query returns the count. Issue #307: edge-liveness.sh now calls
# schema-describe to read reserved labels, so every stub must answer both
# subcommands.
make_stub() {
    local stub_file="$1" count="$2" reserved_labels_csv="$3"
    cat > "$stub_file" <<EOF
#!/usr/bin/env bash
case "\$1" in
    schema-describe)
        IFS=',' read -ra reserved <<<"${reserved_labels_csv}"
        printf '{"edges":['
        first=1
        for r in "\${reserved[@]}"; do
            [ -z "\$r" ] && continue
            [ \$first -eq 1 ] || printf ','
            printf '{"label":"%s","provenance":"reserved"}' "\$r"
            first=0
        done
        printf ']}'
        ;;
    query)
        printf '{\n  "rows": [\n    {\n      "n": ${count}\n    }\n  ]\n}\n'
        ;;
    *)
        echo "stub: unknown subcommand: \$1" >&2
        exit 2
        ;;
esac
EOF
    chmod +x "$stub_file"
}

# --- 3. happy path — stub cfdb reports non-zero for every label → exit 0 ---
make_stub "$TMP/cfdb-all-present" 7 ""
assert_exit "happy path: every label has ≥1 instance → exit 0" 0 \
    env CFDB_BIN="$TMP/cfdb-all-present" CFDB_DB="$TMP/db" \
    CFDB_KEYSPACE="dummy" "$HARNESS"

# --- 4. sad path — stub cfdb reports zero for every label → exit 1 ---
make_stub "$TMP/cfdb-all-zero" 0 ""
assert_exit "sad path: every label has 0 instances → exit 1" 1 \
    env CFDB_BIN="$TMP/cfdb-all-zero" CFDB_DB="$TMP/db" \
    CFDB_KEYSPACE="dummy" "$HARNESS"

# --- 5. mixed path — one label zero, rest present → exit 1 ---
cat > "$TMP/cfdb-one-zero" <<'EOF'
#!/usr/bin/env bash
case "$1" in
    schema-describe)
        printf '{"edges":[]}'
        ;;
    query)
        # Return 0 only when the query names REGISTERS_PARAM, non-zero otherwise.
        # The query string is the last argument to `cfdb query`; grep it out.
        for arg in "$@"; do last="$arg"; done
        if printf '%s' "$last" | grep -q 'REGISTERS_PARAM'; then
            printf '{\n  "rows": [\n    {\n      "n": 0\n    }\n  ]\n}\n'
        else
            printf '{\n  "rows": [\n    {\n      "n": 3\n    }\n  ]\n}\n'
        fi
        ;;
    *)
        exit 2
        ;;
esac
EOF
chmod +x "$TMP/cfdb-one-zero"
assert_exit "mixed path: one dormant label → exit 1" 1 \
    env CFDB_BIN="$TMP/cfdb-one-zero" CFDB_DB="$TMP/db" \
    CFDB_KEYSPACE="dummy" "$HARNESS"
# And the stderr names that specific label.
if grep -q '^  - REGISTERS_PARAM$' "$TMP/stderr"; then
    pass=$((pass + 1))
    echo "PASS: mixed path names the specific dormant label on stderr"
else
    fail=$((fail + 1))
    echo "FAIL: mixed path did not name REGISTERS_PARAM on stderr" >&2
    sed 's/^/    /' "$TMP/stderr" >&2
fi

# --- 6. usage — missing CFDB_BIN → exit 2 ---
assert_exit "missing cfdb binary → exit 2" 2 \
    env CFDB_BIN="$TMP/does-not-exist" CFDB_DB="$TMP/db" \
    CFDB_KEYSPACE="dummy" "$HARNESS"

# --- 7. issue #307 — reserved label suppression (happy path) ---
# Schema describer reports EQUIVALENT_TO as reserved. Every label query
# returns 0. The script must skip EQUIVALENT_TO from the dormant list AND
# every other label is also dormant — but the test only asserts the
# suppression mechanic. We make every label reserved so all 18 are tagged.
all_18="IN_CRATE,IN_MODULE,HAS_FIELD,HAS_VARIANT,HAS_PARAM,TYPE_OF,IMPLEMENTS,IMPLEMENTS_FOR,RETURNS,BELONGS_TO,CALLS,INVOKES_AT,EXPOSES,REGISTERS_PARAM,LABELED_AS,CANONICAL_FOR,EQUIVALENT_TO,REFERENCED_BY"
make_stub "$TMP/cfdb-all-reserved" 0 "$all_18"
assert_exit "issue #307: every label reserved + zero counts → exit 0" 0 \
    env CFDB_BIN="$TMP/cfdb-all-reserved" CFDB_DB="$TMP/db" \
    CFDB_KEYSPACE="dummy" "$HARNESS"
if grep -q 'EQUIVALENT_TO.*0 (reserved)' "$TMP/stdout"; then
    pass=$((pass + 1))
    echo "PASS: reserved label is annotated '(reserved)' in informational table"
else
    fail=$((fail + 1))
    echo "FAIL: reserved annotation missing from stdout" >&2
    sed 's/^/    /' "$TMP/stdout" >&2
fi

# --- 8. issue #307 — narrow suppression (sad path) ---
# Only EQUIVALENT_TO reserved. Every label still returns 0. The 17 non-reserved
# labels remain dormant → exit 1. Forbidden move 5 enforcement: tagging only
# EQUIVALENT_TO must NOT silence the others.
make_stub "$TMP/cfdb-only-eq-reserved" 0 "EQUIVALENT_TO"
assert_exit "issue #307: only EQUIVALENT_TO reserved + others zero → exit 1" 1 \
    env CFDB_BIN="$TMP/cfdb-only-eq-reserved" CFDB_DB="$TMP/db" \
    CFDB_KEYSPACE="dummy" "$HARNESS"
if grep -q '^  - EQUIVALENT_TO$' "$TMP/stderr"; then
    fail=$((fail + 1))
    echo "FAIL: EQUIVALENT_TO appeared in MISSING — narrow suppression broken" >&2
    sed 's/^/    /' "$TMP/stderr" >&2
else
    pass=$((pass + 1))
    echo "PASS: EQUIVALENT_TO suppressed from MISSING; other dormants reported"
fi
if grep -q '^  - CALLS$' "$TMP/stderr"; then
    pass=$((pass + 1))
    echo "PASS: non-reserved CALLS still reported as dormant (forbidden move 5)"
else
    fail=$((fail + 1))
    echo "FAIL: CALLS missing from stderr — suppression too wide" >&2
    sed 's/^/    /' "$TMP/stderr" >&2
fi

echo
echo "$pass passed, $fail failed"
[ "$fail" -eq 0 ]

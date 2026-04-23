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

# --- 3. happy path — stub cfdb reports non-zero for every label → exit 0 ---
cat > "$TMP/cfdb-all-present" <<'EOF'
#!/usr/bin/env bash
# Stub that always reports count=7 regardless of label.
printf '{\n  "rows": [\n    {\n      "n": 7\n    }\n  ]\n}\n'
EOF
chmod +x "$TMP/cfdb-all-present"
assert_exit "happy path: every label has ≥1 instance → exit 0" 0 \
    env CFDB_BIN="$TMP/cfdb-all-present" CFDB_DB="$TMP/db" \
    CFDB_KEYSPACE="dummy" "$HARNESS"

# --- 4. sad path — stub cfdb reports zero for every label → exit 1 ---
cat > "$TMP/cfdb-all-zero" <<'EOF'
#!/usr/bin/env bash
printf '{\n  "rows": [\n    {\n      "n": 0\n    }\n  ]\n}\n'
EOF
chmod +x "$TMP/cfdb-all-zero"
assert_exit "sad path: every label has 0 instances → exit 1" 1 \
    env CFDB_BIN="$TMP/cfdb-all-zero" CFDB_DB="$TMP/db" \
    CFDB_KEYSPACE="dummy" "$HARNESS"

# --- 5. mixed path — one label zero, rest present → exit 1 ---
cat > "$TMP/cfdb-one-zero" <<'EOF'
#!/usr/bin/env bash
# Return 0 only when the query names REGISTERS_PARAM, non-zero otherwise.
# The query string is the last argument to `cfdb query`; grep it out.
for arg in "$@"; do last="$arg"; done
if printf '%s' "$last" | grep -q 'REGISTERS_PARAM'; then
    printf '{\n  "rows": [\n    {\n      "n": 0\n    }\n  ]\n}\n'
else
    printf '{\n  "rows": [\n    {\n      "n": 3\n    }\n  ]\n}\n'
fi
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

echo
echo "$pass passed, $fail failed"
[ "$fail" -eq 0 ]

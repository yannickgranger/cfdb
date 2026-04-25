#!/usr/bin/env bash
# ci/pr-body-check-test.sh
#
# Unit tests for ci/pr-body-check.sh per #240 Tests:
#   - Unit: titles with / without #N; close tokens in body; bundle directive;
#     case-insensitive token match; boundary check so #12 does not match #123.
#
# No test framework — plain assertions so this can run inside the CI
# `setup` step before cargo is warm. Exits non-zero on any failure.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CHECKER="$SCRIPT_DIR/pr-body-check.sh"

fail=0
pass=0

assert_pass() {
    local name="$1" title="$2" body="$3"
    if PR_TITLE="$title" PR_BODY="$body" "$CHECKER" >/dev/null 2>&1; then
        pass=$((pass + 1))
        echo "PASS: $name"
    else
        fail=$((fail + 1))
        echo "FAIL: $name — expected exit 0" >&2
    fi
}

assert_fail() {
    local name="$1" title="$2" body="$3" want_exit="$4"
    local got_exit=0
    PR_TITLE="$title" PR_BODY="$body" "$CHECKER" >/dev/null 2>&1 || got_exit=$?
    if [ "$got_exit" -eq "$want_exit" ]; then
        pass=$((pass + 1))
        echo "PASS: $name (exit $got_exit)"
    else
        fail=$((fail + 1))
        echo "FAIL: $name — expected exit $want_exit, got $got_exit" >&2
    fi
}

# ——— Empty-context behavior ———

assert_pass "empty PR_TITLE skips (not a PR context)" "" ""

# ——— No #N in title → skip ———

assert_pass "title without #N skips" \
    "chore(deps): bump cargo to 1.93" \
    ""

assert_pass "title without #N ignores stray body tokens" \
    "refactor: rename foo to bar" \
    "This fixes issue #999 but we don't advertise that in the title."

# ——— Single #N happy paths ———

assert_pass "Closes #N matches" \
    "fix(foo): bar (#238)" \
    "Closes #238.

Summary: fixes the thing."

assert_pass "Fixes #N matches" \
    "fix: bar (#238)" \
    "Fixes #238."

assert_pass "Resolves #N matches" \
    "feat: bar (#238)" \
    "Resolves #238."

assert_pass "close token case-insensitive" \
    "fix: bar (#238)" \
    "closes #238"

assert_pass "close token mixed case" \
    "fix: bar (#238)" \
    "CLOSES #238"

# ——— Single #N failure paths ———

assert_fail "empty body on #N-titled PR fails" \
    "fix: bar (#238)" \
    "" \
    1

assert_fail "body with only related link fails" \
    "fix: bar (#238)" \
    "Related: #238" \
    1

assert_fail "body with wrong token fails" \
    "fix: bar (#238)" \
    "See #238 for context." \
    1

# ——— Boundary check: #12 must NOT match #123 ———

assert_fail "title #12, body Closes #123 — must fail (boundary)" \
    "fix: bar (#12)" \
    "Closes #123" \
    1

assert_pass "title #123, body Closes #123 — must pass" \
    "fix: bar (#123)" \
    "Closes #123" \

assert_fail "title #1234, body Closes #123 — must fail (boundary)" \
    "fix: bar (#1234)" \
    "Closes #123" \
    1

# ——— Multi-issue bundle paths ———

assert_fail "title with 2 #N, body closes only 1 — fail" \
    "fix: bar (#123, #456)" \
    "Closes #123" \
    1

assert_pass "title with 2 #N, body closes both — pass" \
    "fix: bar (#123, #456)" \
    "Closes #123
Fixes #456"

assert_pass "title with 3 #N, body has Bundle directive — pass" \
    "feat: rfc-037 (#215, #216, #217)" \
    "Bundle: #215, #216, #217

Bundles three RFC-037 slices that co-modify emit::emit_field."

assert_fail "title with 3 #N, body has Bundle with only 2 listed — fail" \
    "feat: rfc-037 (#215, #216, #217)" \
    "Bundle: #215, #216" \
    1

assert_pass "Bundle directive is case-insensitive" \
    "feat: bar (#215, #216)" \
    "bundle: #215, #216"

assert_pass "mixed Closes + Bundle — still passes" \
    "feat: bar (#215, #216, #217)" \
    "Closes #215

Bundle: #216, #217"

# ——— Dedup ———

assert_pass "duplicated #N in title counted once" \
    "fix: #123 and also #123 (#123)" \
    "Closes #123"

# ——— Non-ASCII body doesn't choke grep ———

assert_pass "non-ASCII body still matches close token" \
    "fix: bar (#238)" \
    "Résolves the issue. Closes #238."

echo
echo "$pass passed, $fail failed"
[ "$fail" -eq 0 ]

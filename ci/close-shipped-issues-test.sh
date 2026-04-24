#!/usr/bin/env bash
# ci/close-shipped-issues-test.sh
#
# Unit tests for ci/close-shipped-issues.sh per #240 Tests:
#   - Unit: shell-test against a fixture git log; assert the grep-match
#     heuristic flags known-shipped + doesn't flag genuinely open.
#
# Builds a throwaway git repo inside $TMP with synthetic commits naming
# known issue numbers, then feeds issue numbers on stdin and inspects
# stdout for expected CANDIDATE blocks.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CHECKER="$SCRIPT_DIR/close-shipped-issues.sh"

fail=0
pass=0

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

# Build a fixture git repo with synthetic main + develop branches.
FIXTURE="$TMP/fixture"
mkdir -p "$FIXTURE"
(
    cd "$FIXTURE"
    git init -q -b main .
    git config user.email "test@example.com"
    git config user.name  "Test"
    echo "init" > README.md
    git add README.md
    git commit -q -m "chore: init"

    # Simulate a merged PR closing #100
    echo "1" > a.txt
    git add a.txt
    git commit -q -m "feat(foo): add bar (#100)

Closes #100."

    # Simulate a bundle PR closing #200 and #201 (body mentions both)
    echo "2" > b.txt
    git add b.txt
    git commit -q -m "feat: bundle work (#200, #201)

Bundle: #200, #201"

    # Simulate a commit that mentions #300 in passing (not a close)
    echo "3" > c.txt
    git add c.txt
    git commit -q -m "chore: maintenance

See #300 for context; not closed by this commit."

    # A develop-only commit closing #400
    git checkout -q -b develop
    echo "4" > d.txt
    git add d.txt
    git commit -q -m "fix: develop-only change (#400)

Closes #400."

    # Set up "remote" refs so origin/main + origin/develop resolve.
    # We alias the local branches to origin/* so `git rev-parse origin/main`
    # works without a real remote. `git update-ref` is the low-level tool.
    git update-ref refs/remotes/origin/main "$(git rev-parse main)"
    git update-ref refs/remotes/origin/develop "$(git rev-parse develop)"
)

# Helper — run the checker inside the fixture repo, capture stdout.
run_checker() {
    local input="$1"
    ( cd "$FIXTURE" && printf '%s\n' "$input" | "$CHECKER" 2>/dev/null )
}

assert_candidate() {
    local name="$1" input="$2" expected_issue="$3"
    local out
    out="$(run_checker "$input")"
    if printf '%s' "$out" | grep -qE "^CANDIDATE #${expected_issue}:"; then
        pass=$((pass + 1))
        echo "PASS: $name"
    else
        fail=$((fail + 1))
        echo "FAIL: $name — expected CANDIDATE #${expected_issue}, got:" >&2
        printf '%s\n' "$out" >&2
    fi
}

assert_no_candidate() {
    local name="$1" input="$2" issue="$3"
    local out
    out="$(run_checker "$input")"
    if printf '%s' "$out" | grep -qE "^CANDIDATE #${issue}:"; then
        fail=$((fail + 1))
        echo "FAIL: $name — did not expect CANDIDATE #${issue}, got:" >&2
        printf '%s\n' "$out" >&2
    else
        pass=$((pass + 1))
        echo "PASS: $name"
    fi
}

# ——— Happy path: shipped issues surface as candidates ———

assert_candidate "#100 shipped on main is a candidate" "100" "100"
assert_candidate "#400 shipped on develop is a candidate" "400" "400"

# Bundle PR — both referenced issues surface.
assert_candidate "#200 bundled-shipped is a candidate" "200" "200"
assert_candidate "#201 bundled-shipped is a candidate" "201" "201"

# ——— Not-shipped → no candidate ———

assert_no_candidate "#999 not shipped — no candidate" "999" "999"

# ——— "See #300" style mention → IS a candidate (conservative) ———
# The heuristic surfaces any #N match; a human confirms scope before
# closing. Per issue body: "NOT fully automatic because some #N refs in
# commits are cross-links, not closes."
assert_candidate "#300 cross-linked is still a candidate (conservative)" "300" "300"

# ——— Boundary: #10 in input must not match commit mentioning #100 ———
assert_no_candidate "#10 must not match commit mentioning #100 (boundary)" "10" "10"

# ——— Boundary: #1000 in input must not match commit mentioning #100 ———
assert_no_candidate "#1000 must not match commit mentioning #100 (boundary)" "1000" "1000"

# ——— Input hygiene ———

# Blank lines + comments — must be ignored silently.
out="$(run_checker "
# a comment
100

   # indented comment
400
")"
if printf '%s' "$out" | grep -qE "^CANDIDATE #100:" && \
   printf '%s' "$out" | grep -qE "^CANDIDATE #400:"; then
    pass=$((pass + 1))
    echo "PASS: blank + comment lines ignored"
else
    fail=$((fail + 1))
    echo "FAIL: blank + comment lines — got:" >&2
    printf '%s\n' "$out" >&2
fi

# Bare digits and `#123` form both accepted.
out="$(run_checker "100
#400")"
if printf '%s' "$out" | grep -qE "^CANDIDATE #100:" && \
   printf '%s' "$out" | grep -qE "^CANDIDATE #400:"; then
    pass=$((pass + 1))
    echo "PASS: bare digits and #N form both accepted"
else
    fail=$((fail + 1))
    echo "FAIL: bare / #N form accepted — got:" >&2
    printf '%s\n' "$out" >&2
fi

# Non-numeric input — warn + skip, don't fail.
if ( cd "$FIXTURE" && printf 'foo\n100\n' | "$CHECKER" >/dev/null 2>&1 ); then
    pass=$((pass + 1))
    echo "PASS: non-numeric input is warning, not fatal"
else
    fail=$((fail + 1))
    echo "FAIL: non-numeric input — script exited non-zero" >&2
fi

# Empty stdin — zero candidates, exit 0.
if ( cd "$FIXTURE" && printf '' | "$CHECKER" >/dev/null 2>&1 ); then
    pass=$((pass + 1))
    echo "PASS: empty stdin — exit 0"
else
    fail=$((fail + 1))
    echo "FAIL: empty stdin — exit non-zero" >&2
fi

# ——— --branches override ———
out="$( cd "$FIXTURE" && printf '400\n' | "$CHECKER" --branches "origin/main" 2>/dev/null )"
if printf '%s' "$out" | grep -qE "^CANDIDATE #400:"; then
    fail=$((fail + 1))
    echo "FAIL: --branches origin/main should not surface #400 (develop-only)" >&2
else
    pass=$((pass + 1))
    echo "PASS: --branches restricts lookup to named refs"
fi

# Help flag exits 0.
if "$CHECKER" --help >/dev/null 2>&1; then
    pass=$((pass + 1))
    echo "PASS: --help exits 0"
else
    fail=$((fail + 1))
    echo "FAIL: --help exit non-zero" >&2
fi

# Unknown flag exits 2.
got_exit=0
"$CHECKER" --bogus >/dev/null 2>&1 || got_exit=$?
if [ "$got_exit" -eq 2 ]; then
    pass=$((pass + 1))
    echo "PASS: unknown flag exits 2"
else
    fail=$((fail + 1))
    echo "FAIL: unknown flag exit expected 2, got $got_exit" >&2
fi

echo
echo "$pass passed, $fail failed"
[ "$fail" -eq 0 ]

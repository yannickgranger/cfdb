#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CHECKER="$SCRIPT_DIR/close-shipped-issues.sh"

fail=0
pass=0

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

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

    echo "1" > a.txt
    git add a.txt
    git commit -q -m "feat(foo): add bar (#100)

Closes #100."

    echo "2" > b.txt
    git add b.txt
    git commit -q -m "feat: bundle work (#200, #201)

Bundle: #200, #201"

    echo "3" > c.txt
    git add c.txt
    git commit -q -m "chore: maintenance

See #300 for context; not closed by this commit."

    git checkout -q -b develop
    echo "4" > d.txt
    git add d.txt
    git commit -q -m "fix: develop-only change (#400)

Closes #400."

    git update-ref refs/remotes/origin/main "$(git rev-parse main)"
    git update-ref refs/remotes/origin/develop "$(git rev-parse develop)"
)

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

assert_candidate "#100 shipped on main is a candidate" "100" "100"
assert_candidate "#400 shipped on develop is a candidate" "400" "400"

assert_candidate "#200 bundled-shipped is a candidate" "200" "200"
assert_candidate "#201 bundled-shipped is a candidate" "201" "201"

assert_no_candidate "#999 not shipped — no candidate" "999" "999"

assert_candidate "#300 cross-linked is still a candidate (conservative)" "300" "300"

assert_no_candidate "#10 must not match commit mentioning #100 (boundary)" "10" "10"

assert_no_candidate "#1000 must not match commit mentioning #100 (boundary)" "1000" "1000"

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

if ( cd "$FIXTURE" && printf 'foo\n100\n' | "$CHECKER" >/dev/null 2>&1 ); then
    pass=$((pass + 1))
    echo "PASS: non-numeric input is warning, not fatal"
else
    fail=$((fail + 1))
    echo "FAIL: non-numeric input — script exited non-zero" >&2
fi

if ( cd "$FIXTURE" && printf '' | "$CHECKER" >/dev/null 2>&1 ); then
    pass=$((pass + 1))
    echo "PASS: empty stdin — exit 0"
else
    fail=$((fail + 1))
    echo "FAIL: empty stdin — exit non-zero" >&2
fi

out="$( cd "$FIXTURE" && printf '400\n' | "$CHECKER" --branches "origin/main" 2>/dev/null )"
if printf '%s' "$out" | grep -qE "^CANDIDATE #400:"; then
    fail=$((fail + 1))
    echo "FAIL: --branches origin/main should not surface #400 (develop-only)" >&2
else
    pass=$((pass + 1))
    echo "PASS: --branches restricts lookup to named refs"
fi

if "$CHECKER" --help >/dev/null 2>&1; then
    pass=$((pass + 1))
    echo "PASS: --help exits 0"
else
    fail=$((fail + 1))
    echo "FAIL: --help exit non-zero" >&2
fi

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

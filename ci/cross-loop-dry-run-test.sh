#!/usr/bin/env bash
# ci/cross-loop-dry-run-test.sh
#
# Unit test for ci/cross-loop.sh per Issue #70 Tests: "workflow YAML
# lint" / dry-run sanity. Exercises the two behavioural branches
# (pass → no-op, fail → drift-issue preview) without touching git
# remote or Gitea API.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

PASS_FILE="$TMP/.pass"; FAIL_FILE="$TMP/.fail"
: >"$PASS_FILE"; : >"$FAIL_FILE"
mark_pass() { echo "$1" >> "$PASS_FILE"; }
mark_fail() { echo "$1" >> "$FAIL_FILE"; }

(
    cd "$TMP"
    git init --initial-branch=develop --bare companion.git >/dev/null
    git init --initial-branch=develop source >/dev/null
    cd source
    git config user.email 'test@local'
    git config user.name 'test'
    echo "cross-loop-test" > README.md
    git add README.md
    git commit -q -m "loop fixture"
    git push -q "../companion.git" develop
)
COMPANION_HEAD="$(git -C "$TMP/companion.git" rev-parse HEAD)"

# Scenario 1 — stub cross-dogfood exit 0 → silent pass, no issue.
(
    cd "$TMP"
    cp -a "$REPO_ROOT" "local"
    cd local
    mv ci/cross-dogfood.sh ci/cross-dogfood.sh.real
    cat > ci/cross-dogfood.sh <<'STUB'
#!/usr/bin/env bash
echo "stub cross-dogfood: clean (exit 0)"
exit 0
STUB
    chmod +x ci/cross-dogfood.sh
    out="$(DRY_RUN=1 \
        COMPANION_REPO="companion" \
        COMPANION_URL_BASE="file://$TMP" \
        BASE_BRANCH=develop \
        bash ci/cross-loop.sh 2>&1)"
    mv ci/cross-dogfood.sh.real ci/cross-dogfood.sh
    if printf '%s' "$out" | grep -q "closed-loop clean" \
       && ! printf '%s' "$out" | grep -q "would open issue"; then
        mark_pass "$?"
        echo "PASS: scenario 1 — pass path is silent (no drift issue)"
    else
        mark_fail "$?"
        echo "FAIL: scenario 1 — expected clean pass, got:"
        printf '%s\n' "$out" | sed 's/^/  /'
    fi
)

# Scenario 2 — stub cross-dogfood exit 30 → drift issue preview.
(
    cd "$TMP"
    rm -rf local
    cp -a "$REPO_ROOT" "local"
    cd local
    mv ci/cross-dogfood.sh ci/cross-dogfood.sh.real
    cat > ci/cross-dogfood.sh <<'STUB'
#!/usr/bin/env bash
echo "stub cross-dogfood: FAIL simulated (exit 30)"
exit 30
STUB
    chmod +x ci/cross-dogfood.sh
    out="$(DRY_RUN=1 \
        COMPANION_REPO="companion" \
        COMPANION_URL_BASE="file://$TMP" \
        BASE_BRANCH=develop \
        bash ci/cross-loop.sh 2>&1 || true)"
    mv ci/cross-dogfood.sh.real ci/cross-dogfood.sh
    if printf '%s' "$out" | grep -q "would open issue 'cross-drift-" \
       && printf '%s' "$out" | grep -q "Exit code: \`30\`" \
       && printf '%s' "$out" | grep -q "develop HEAD, NOT pinned"; then
        mark_pass "$?"
        echo "PASS: scenario 2 — drift preview names HEAD + exit code"
    else
        mark_fail "$?"
        echo "FAIL: scenario 2 — expected drift preview with HEAD wording:"
        printf '%s\n' "$out" | sed 's/^/  /'
    fi
)

echo
pass=$(wc -l < "$PASS_FILE"); fail=$(wc -l < "$FAIL_FILE")
echo "$pass passed, $fail failed"
[ "$fail" -eq 0 ]

#!/usr/bin/env bash
# ci/cross-bump-dry-run-test.sh
#
# Unit test for ci/cross-bump.sh per Issue #67 Tests: "bump-script
# dry-run against a local fixture directory". Exercises the
# orchestration without touching git remote or the Gitea API.
#
# Strategy: set DRY_RUN=1, point COMPANION_REPO at a local temp git
# repo that mimics a companion with a known HEAD SHA. Assert that
# the script exits 0 and the diff it would produce is correct.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

# Counters are written to files so subshells (each scenario runs in
# its own `(…)`) aggregate up to the parent.
PASS_FILE="$TMP/.pass"
FAIL_FILE="$TMP/.fail"
: >"$PASS_FILE"; : >"$FAIL_FILE"
mark_pass() { echo "$1" >> "$PASS_FILE"; }
mark_fail() { echo "$1" >> "$FAIL_FILE"; }

# Spin up a bare "companion.git" repo at TMP/companion.git so
# `git ls-remote "${URL_BASE}/companion.git" refs/heads/develop` (which
# is exactly how cross-bump.sh forms the URL) resolves a deterministic
# HEAD SHA.
(
    cd "$TMP"
    git init --initial-branch=develop --bare companion.git >/dev/null
    git init --initial-branch=develop source >/dev/null
    cd source
    git config user.email 'test@local'
    git config user.name 'test'
    echo "cross-bump-test" > README.md
    git add README.md
    git commit -q -m "cross-bump fixture"
    git push -q "../companion.git" develop
)
COMPANION_HEAD="$(git -C "$TMP/companion.git" rev-parse HEAD)"

# Scenario 1 — pin already at HEAD should be a no-op.
(
    cd "$TMP"
    cp -a "$REPO_ROOT" "local"
    # Prepare the local fixture to already pin the companion HEAD.
    sed -i -E "s|^(\s*sha\s*=\s*)\".*\"|\1\"${COMPANION_HEAD}\"|" local/.cfdb/cross-fixture.toml
    cd local
    out="$(DRY_RUN=1 \
        COMPANION_REPO="companion" \
        COMPANION_URL_BASE="file://$TMP" \
        BASE_BRANCH=develop \
        bash ci/cross-bump.sh 2>&1)"
    if printf '%s' "$out" | grep -q "pin already at HEAD"; then
        mark_pass "$?"
        echo "PASS: scenario 1 — already-at-HEAD is a no-op"
    else
        mark_fail "$?"
        echo "FAIL: scenario 1 — expected 'pin already at HEAD' in output:"
        printf '%s\n' "$out" | sed 's/^/  /'
    fi
)

# Scenario 2 — stale pin → DRY_RUN prints the bump diff it would apply.
(
    cd "$TMP"
    rm -rf local
    cp -a "$REPO_ROOT" "local"
    # Leave the existing sha alone — it will differ from COMPANION_HEAD,
    # which is the bump trigger.
    cd local
    # Stub out the actual cross-dogfood run so this test does not try
    # to clone a real cfdb-checkable tree for 1870 files.
    mv ci/cross-dogfood.sh ci/cross-dogfood.sh.real
    cat > ci/cross-dogfood.sh <<'STUB'
#!/usr/bin/env bash
echo "stub cross-dogfood: pretending exit 0 for dry-run test"
exit 0
STUB
    chmod +x ci/cross-dogfood.sh
    out="$(DRY_RUN=1 \
        COMPANION_REPO="companion" \
        COMPANION_URL_BASE="file://$TMP" \
        BASE_BRANCH=develop \
        bash ci/cross-bump.sh 2>&1 || true)"
    mv ci/cross-dogfood.sh.real ci/cross-dogfood.sh
    if printf '%s' "$out" | grep -q "would push branch" \
       && printf '%s' "$out" | grep -q "${COMPANION_HEAD}"; then
        mark_pass "$?"
        echo "PASS: scenario 2 — DRY_RUN shows intended bump + new SHA"
    else
        mark_fail "$?"
        echo "FAIL: scenario 2 — expected 'would push branch' + new SHA in output:"
        printf '%s\n' "$out" | sed 's/^/  /'
    fi
)

# Scenario 3 — stubbed failure → DRY_RUN prints the cross-drift body.
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
        bash ci/cross-bump.sh 2>&1 || true)"
    mv ci/cross-dogfood.sh.real ci/cross-dogfood.sh
    if printf '%s' "$out" | grep -q "would open issue 'cross-drift-" \
       && printf '%s' "$out" | grep -q "Exit code: \`30\`"; then
        mark_pass "$?"
        echo "PASS: scenario 3 — DRY_RUN emits cross-drift issue body"
    else
        mark_fail "$?"
        echo "FAIL: scenario 3 — expected drift issue preview with exit=30:"
        printf '%s\n' "$out" | sed 's/^/  /'
    fi
)

echo
pass=$(wc -l < "$PASS_FILE"); fail=$(wc -l < "$FAIL_FILE"); echo "$pass passed, $fail failed"
[ "$fail" -eq 0 ]

#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

PASS_FILE="$TMP/.pass"
FAIL_FILE="$TMP/.fail"
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
    echo "cross-bump-test" > README.md
    git add README.md
    git commit -q -m "cross-bump fixture"
    git push -q "../companion.git" develop
)
COMPANION_HEAD="$(git -C "$TMP/companion.git" rev-parse HEAD)"

(
    cd "$TMP"
    cp -a "$REPO_ROOT" "local"
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

(
    cd "$TMP"
    rm -rf local
    cp -a "$REPO_ROOT" "local"
    cd local
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

(
    cd "$TMP"
    rm -rf local
    cp -a "$REPO_ROOT" "local"
    rm -rf local/.git
    git init -q local
    git -C local remote add origin "file://$TMP/nowhere.git"
    cd local
    mkdir -p "$TMP/home"
    set +e
    out="$(HOME="$TMP/home" XDG_CONFIG_HOME="$TMP/home/.config" \
        GITHUB_TOKEN=stub-actions-token \
        API_BASE="http://127.0.0.1:1/api/v1" \
        GITHUB_REPOSITORY=yg/cfdb \
        COMPANION_REPO="companion" \
        COMPANION_URL_BASE="file://$TMP" \
        BASE_BRANCH=develop \
        bash ci/cross-bump.sh 2>&1)"
    rc=$?
    set -e
    if [ "$rc" -ne 0 ] && printf '%s' "$out" | grep -q "BUMP_PAT is unset"; then
        mark_pass "$?"
        echo "PASS: scenario 4 — absent BUMP_PAT refuses (exit $rc)"
    else
        mark_fail "$?"
        echo "FAIL: scenario 4 — expected a named refusal, got exit $rc:"
        printf '%s\n' "$out" | sed 's/^/  /'
    fi
)

(
    cd "$REPO_ROOT"
    CREATING_CALLS=3
    creating="$(grep -n -- '-X POST' ci/cross-bump.sh ci/cross-loop.sh || true)"
    n="$(printf '%s\n' "$creating" | grep -c . || true)"
    if [ "$n" -ne "$CREATING_CALLS" ]; then
        mark_fail "$?"
        echo "FAIL: scenario 5 — $n creating call(s) found, $CREATING_CALLS declared: a call that left the count is invisible to this scenario, and one that joined it was never fenced"
        printf '%s\n' "$creating" | sed 's/^/  /'
    elif printf '%s\n' "$creating" | grep -q 'GITHUB_TOKEN'; then
        mark_fail "$?"
        echo "FAIL: scenario 5 — a creating call (-X POST) still carries the Actions token:"
        printf '%s\n' "$creating" | grep 'GITHUB_TOKEN' | sed 's/^/  /'
    elif [ "$(printf '%s\n' "$creating" | grep -c 'BUMP_PAT')" -ne "$n" ]; then
        mark_fail "$?"
        echo "FAIL: scenario 5 — $n creating call(s), only $(printf '%s\n' "$creating" | grep -c 'BUMP_PAT') carry BUMP_PAT"
    else
        mark_pass "$?"
        echo "PASS: scenario 5 — all $n creating call(s) use the non-Actions token"
    fi
)

echo
pass=$(wc -l < "$PASS_FILE"); fail=$(wc -l < "$FAIL_FILE"); echo "$pass passed, $fail failed"
[ "$fail" -eq 0 ]

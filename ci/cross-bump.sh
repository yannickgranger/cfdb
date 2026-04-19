#!/usr/bin/env bash
# ci/cross-bump.sh
#
# Weekly cross-fixture bump orchestrator (RFC-033 §3.3, Issue #67).
# Invoked by `.gitea/workflows/cross-bump.yml` on Monday 06:00 UTC and
# by manual `workflow_dispatch` for debugging.
#
# Flow:
#   1. Resolve companion `develop` HEAD SHA.
#   2. If HEAD == current pin: nothing to do.
#   3. Otherwise, run `ci/cross-dogfood.sh` with COMPANION_SHA=HEAD.
#      - exit 0: update `.cfdb/cross-fixture.toml`, push a branch,
#        open a bump PR targeting `develop`.
#      - non-zero: open (or update) a `cross-drift-YYYY-WW` issue with
#        the failing invocation, exit code, and log tail per the
#        exit-code contract in docs/cross-fixture-bump.md §1.4.
#
# Env required:
#   GITHUB_TOKEN       — Gitea API token (supplied by Actions secret)
#   GITHUB_REPOSITORY  — this repo's owner/name (supplied by runtime)
#
# Env optional:
#   COMPANION_REPO     (default: yg/graph-specs-rust)
#   COMPANION_URL_BASE (default: https://agency.lab:3000)
#   BASE_BRANCH        (default: develop)
#   DRY_RUN            — if set, skip git push + API calls (unit-test mode)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

COMPANION_REPO="${COMPANION_REPO:-yg/graph-specs-rust}"
COMPANION_URL_BASE="${COMPANION_URL_BASE:-https://agency.lab:3000}"
API_BASE="${API_BASE:-${COMPANION_URL_BASE}/api/v1}"
BASE_BRANCH="${BASE_BRANCH:-develop}"
DRY_RUN="${DRY_RUN:-}"

if [ -z "$DRY_RUN" ]; then
    : "${GITHUB_TOKEN:?GITHUB_TOKEN required (unset DRY_RUN for local testing)}"
    : "${GITHUB_REPOSITORY:?GITHUB_REPOSITORY required}"
fi

log() { printf 'cross-bump: %s\n' "$*"; }

# ── 1. Resolve companion HEAD SHA ────────────────────────────────────
if [ -n "${GITHUB_TOKEN:-}" ]; then
    git config --global url."https://oauth2:${GITHUB_TOKEN}@agency.lab:3000/".insteadOf "https://agency.lab:3000/"
fi

HEAD_SHA="$(git ls-remote "${COMPANION_URL_BASE}/${COMPANION_REPO}.git" "refs/heads/${BASE_BRANCH}" | awk '{print $1}')"
if ! printf '%s' "$HEAD_SHA" | grep -Eq '^[0-9a-f]{40}$'; then
    log "FATAL: could not resolve ${COMPANION_REPO} ${BASE_BRANCH} HEAD"
    exit 1
fi

CURRENT_SHA="$("$SCRIPT_DIR/read-cross-fixture-sha.sh")"
log "current pin: ${CURRENT_SHA}"
log "HEAD:        ${HEAD_SHA}"

if [ "$HEAD_SHA" = "$CURRENT_SHA" ]; then
    log "pin already at HEAD — nothing to do"
    exit 0
fi

# ── 2. Run cross-dogfood against HEAD ────────────────────────────────
LOGFILE="$(mktemp)"
set +e
COMPANION_SHA="$HEAD_SHA" bash "$SCRIPT_DIR/cross-dogfood.sh" 2>&1 | tee "$LOGFILE"
DOGFOOD_EXIT="${PIPESTATUS[0]}"
set -e
log "cross-dogfood.sh exit=${DOGFOOD_EXIT}"

WEEK="$(date -u +%Y-%V)"
ISSUE_TITLE="cross-drift-${WEEK}"

# ── 3a. Failure path — open (or skip) a cross-drift issue ────────────
if [ "$DOGFOOD_EXIT" -ne 0 ]; then
    LOG_TAIL="$(tail -50 "$LOGFILE")"
    body_json="$(python3 - <<'PY' "$COMPANION_REPO" "$HEAD_SHA" "$DOGFOOD_EXIT" "$LOG_TAIL"
import json, sys
companion, sha, code, tail = sys.argv[1:]
body = f"""**Automated cross-drift report** (weekly bump cron — see `docs/cross-fixture-bump.md`).

- Failing invocation: `COMPANION_SHA={sha} ci/cross-dogfood.sh`
- Companion: `{companion}` @ `{sha}`
- Exit code: `{code}` (see `docs/cross-fixture-bump.md` §1.4)

```
{tail}
```

Per runbook §6, the next PR in this repo is blocked until this issue is resolved. Resolution paths:
1. Fix the finding at companion and bump the pin (runbook §3)
2. Narrow the local rule shape (runbook §5)
3. Pin-hold with open fix PR referenced here
"""
print(json.dumps(body))
PY
)"

    if [ -n "$DRY_RUN" ]; then
        log "DRY_RUN — would open issue '${ISSUE_TITLE}' with body:"
        printf '%s\n' "$body_json" | python3 -c 'import json,sys;print(json.loads(sys.stdin.read()))'
        exit 0
    fi

    # De-dup: skip if already open this week.
    existing="$(curl -sf -H "Authorization: token ${GITHUB_TOKEN}" \
        "${API_BASE}/repos/${GITHUB_REPOSITORY}/issues?state=open&type=issues&q=${ISSUE_TITLE}" \
        | python3 -c "import json,sys; data=json.load(sys.stdin); print(sum(1 for i in data if i.get('title')=='${ISSUE_TITLE}'))" \
        || echo 0)"
    if [ "$existing" -gt 0 ]; then
        log "${ISSUE_TITLE} already open — skipping issue creation"
        exit 0
    fi

    payload="$(python3 -c "import json,sys; print(json.dumps({'title':'${ISSUE_TITLE}','body':json.loads(sys.stdin.read())}))" <<< "$body_json")"
    curl -sf -X POST -H "Authorization: token ${GITHUB_TOKEN}" \
        -H "Content-Type: application/json" \
        "${API_BASE}/repos/${GITHUB_REPOSITORY}/issues" \
        -d "$payload" >/dev/null
    log "opened ${ISSUE_TITLE}"
    exit 0
fi

# ── 3b. Success path — open a bump PR ────────────────────────────────
cd "$REPO_ROOT"
BRANCH="chore/cross-bump-${HEAD_SHA:0:12}"
NOW="$(date -u +%Y-%m-%dT%H:%M:%SZ)"

# Update the fixture in-place via sed (anchored on the field names —
# same discipline as read-cross-fixture-sha.sh).
sed -i.bak -E \
    -e "s|^(\s*sha\s*=\s*)\".*\"|\1\"${HEAD_SHA}\"|" \
    -e "s|^(\s*bumped_at\s*=\s*)\".*\"|\1\"${NOW}\"|" \
    -e "s|^(\s*bumped_by\s*=\s*)\".*\"|\1\"cron — cross-bump (Mon 06:00 UTC)\"|" \
    .cfdb/cross-fixture.toml
rm -f .cfdb/cross-fixture.toml.bak

# Re-verify the parser is happy with the new file.
NEW_SHA="$("$SCRIPT_DIR/read-cross-fixture-sha.sh")"
if [ "$NEW_SHA" != "$HEAD_SHA" ]; then
    log "FATAL: post-bump fixture does not round-trip ($NEW_SHA != $HEAD_SHA)"
    exit 1
fi

if [ -n "$DRY_RUN" ]; then
    log "DRY_RUN — bump succeeded; would push branch ${BRANCH} and open PR."
    log "diff:"
    git --no-pager diff -- .cfdb/cross-fixture.toml | sed 's/^/  /'
    exit 0
fi

# Check if branch already exists upstream (prior cron with same SHA).
if git ls-remote --exit-code --heads origin "$BRANCH" >/dev/null 2>&1; then
    log "branch ${BRANCH} already exists upstream — PR likely already open, skipping"
    exit 0
fi

git config user.email "cross-bump-cron@agency.lab"
git config user.name  "cross-bump (Gitea Actions)"
git checkout -b "$BRANCH"
git add .cfdb/cross-fixture.toml
git commit -m "chore: weekly cross-fixture bump → ${HEAD_SHA:0:12}"
git push origin "$BRANCH"

pr_body="$(python3 - <<PY
import json
body = f"""**Automated weekly cross-fixture bump** (see \`docs/cross-fixture-bump.md\` §2).

- Previous pin: \`{"${CURRENT_SHA}"}\`
- New pin: \`{"${HEAD_SHA}"}\`
- Companion: \`{"${COMPANION_REPO}"}\` @ \`{"${BASE_BRANCH}"}\` HEAD
- \`ci/cross-dogfood.sh\` against new pin: **exit 0** ✅

PR-time CI re-runs cross-dogfood against this bump. Merge requires a human reviewer per repo policy.
"""
print(json.dumps(body))
PY
)"

payload="$(python3 -c "import json,sys; print(json.dumps({'title':'chore: weekly cross-fixture bump → ${HEAD_SHA:0:12}','body':json.loads(sys.stdin.read()),'head':'${BRANCH}','base':'${BASE_BRANCH}'}))" <<< "$pr_body")"

curl -sf -X POST -H "Authorization: token ${GITHUB_TOKEN}" \
    -H "Content-Type: application/json" \
    "${API_BASE}/repos/${GITHUB_REPOSITORY}/pulls" \
    -d "$payload" >/dev/null
log "opened bump PR targeting ${BASE_BRANCH}"

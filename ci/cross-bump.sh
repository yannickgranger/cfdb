#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

COMPANION_REPO="${COMPANION_REPO:-yg/graph-specs-rust}"
COMPANION_URL_BASE="${COMPANION_URL_BASE:-https://agency.lab:3000}"
API_BASE="${API_BASE:-${COMPANION_URL_BASE}/api/v1}"
BASE_BRANCH="${BASE_BRANCH:-develop}"
DRY_RUN="${DRY_RUN:-}"

BUMP_PAT="${BUMP_PAT:-}"

if [ -z "$DRY_RUN" ]; then
    : "${GITHUB_TOKEN:?GITHUB_TOKEN required (unset DRY_RUN for local testing)}"
    : "${GITHUB_REPOSITORY:?GITHUB_REPOSITORY required}"
    if [ -z "$BUMP_PAT" ]; then
        printf 'cross-bump: BUMP_PAT is unset, so the PR would be opened with the Actions token and no lane would ever run on it — see cfdb #660. Set the BUMP_PAT secret to a token of a real user with push.\n' >&2
        exit 1
    fi
fi

log() { printf 'cross-bump: %s\n' "$*"; }

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

LOGFILE="$(mktemp)"
set +e
COMPANION_SHA="$HEAD_SHA" bash "$SCRIPT_DIR/cross-dogfood.sh" 2>&1 | tee "$LOGFILE"
DOGFOOD_EXIT="${PIPESTATUS[0]}"
set -e
log "cross-dogfood.sh exit=${DOGFOOD_EXIT}"

WEEK="$(date -u +%Y-%V)"
ISSUE_TITLE="cross-drift-${WEEK}"

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

    existing="$(curl -sf -H "Authorization: token ${GITHUB_TOKEN}" \
        "${API_BASE}/repos/${GITHUB_REPOSITORY}/issues?state=open&type=issues&q=${ISSUE_TITLE}" \
        | python3 -c "import json,sys; data=json.load(sys.stdin); print(sum(1 for i in data if i.get('title')=='${ISSUE_TITLE}'))" \
        || echo 0)"
    if [ "$existing" -gt 0 ]; then
        log "${ISSUE_TITLE} already open — skipping issue creation"
        exit 0
    fi

    payload="$(python3 -c "import json,sys; print(json.dumps({'title':'${ISSUE_TITLE}','body':json.loads(sys.stdin.read())}))" <<< "$body_json")"
    curl -sf -X POST -H "Authorization: token ${BUMP_PAT}" \
        -H "Content-Type: application/json" \
        "${API_BASE}/repos/${GITHUB_REPOSITORY}/issues" \
        -d "$payload" >/dev/null
    log "opened ${ISSUE_TITLE}"
    exit 0
fi

cd "$REPO_ROOT"
BRANCH="chore/cross-bump-${HEAD_SHA:0:12}"
NOW="$(date -u +%Y-%m-%dT%H:%M:%SZ)"

sed -i.bak -E \
    -e "s|^(\s*sha\s*=\s*)\".*\"|\1\"${HEAD_SHA}\"|" \
    -e "s|^(\s*bumped_at\s*=\s*)\".*\"|\1\"${NOW}\"|" \
    -e "s|^(\s*bumped_by\s*=\s*)\".*\"|\1\"cron — cross-bump (Mon 06:00 UTC)\"|" \
    .cfdb/cross-fixture.toml
rm -f .cfdb/cross-fixture.toml.bak

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

branch_exists=0
if git ls-remote --exit-code --heads origin "$BRANCH" >/dev/null 2>&1; then
    branch_exists=1
    open_pr="$(curl -s -H "Authorization: token ${GITHUB_TOKEN}" \
        "${API_BASE}/repos/${GITHUB_REPOSITORY}/pulls?state=open&limit=50" \
        | python3 -c "import json,sys
try:
    print(sum(1 for p in json.load(sys.stdin) if p.get('head',{}).get('ref')=='${BRANCH}'))
except Exception:
    print(0)" 2>/dev/null || echo 0)"
    if [ "${open_pr:-0}" -gt 0 ]; then
        log "branch ${BRANCH} already has an open PR — nothing to do"
        exit 0
    fi
    log "branch ${BRANCH} exists upstream but has NO open PR — opening it"
fi

if [ "$branch_exists" -eq 0 ]; then
    git config user.email "cross-bump-cron@agency.lab"
    git config user.name  "cross-bump (Gitea Actions)"
    git checkout -b "$BRANCH"
    git add .cfdb/cross-fixture.toml
    git commit -m "chore: weekly cross-fixture bump → ${HEAD_SHA:0:12}"
    git push origin "$BRANCH"
fi

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

pr_resp="$(curl -s -w $'\n%{http_code}' -X POST -H "Authorization: token ${BUMP_PAT}" \
    -H "Content-Type: application/json" \
    "${API_BASE}/repos/${GITHUB_REPOSITORY}/pulls" \
    -d "$payload")"
pr_code="$(printf '%s' "$pr_resp" | tail -n1)"
if [ "${pr_code:-0}" -ge 400 ]; then
    log "FATAL: PR creation failed (HTTP ${pr_code}). Response body:"
    printf '%s\n' "$pr_resp" | sed '$d' | sed 's/^/  /'
    log "hint: the workflow must grant 'permissions: pull-requests: write'."
    exit 1
fi
log "opened bump PR targeting ${BASE_BRANCH} (HTTP ${pr_code})"

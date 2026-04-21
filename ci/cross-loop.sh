#!/usr/bin/env bash
# ci/cross-loop.sh
#
# Weekly closed-loop housekeeping (RFC-033 §3.6, Issue #70). Runs
# `ci/cross-dogfood.sh` against the companion at `develop` HEAD —
# NOT the pinned SHA. Purpose: surface drift in the window between
# "companion landed a change" and "our next pin bump" (weekly cron
# at Monday 06:00 or manual PR).
#
# Distinct from ci/cross-bump.sh:
#   - closed-loop tests HEAD regardless of pin state (never no-op on
#     pin == HEAD, since that's the check it's there to make)
#   - on pass: log and exit 0 (no bump, no PR)
#   - on fail: open `cross-drift-YYYY-WW` issue (de-duped)
#
# Env required:
#   GITHUB_TOKEN, GITHUB_REPOSITORY (Gitea Actions provides both)
#
# Env optional:
#   COMPANION_REPO     (default: yg/graph-specs-rust)
#   COMPANION_URL_BASE (default: https://agency.lab:3000)
#   BASE_BRANCH        (default: develop)
#   DRY_RUN            — skip API calls (unit-test mode)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

COMPANION_REPO="${COMPANION_REPO:-yg/graph-specs-rust}"
COMPANION_URL_BASE="${COMPANION_URL_BASE:-https://agency.lab:3000}"
API_BASE="${API_BASE:-${COMPANION_URL_BASE}/api/v1}"
BASE_BRANCH="${BASE_BRANCH:-develop}"
DRY_RUN="${DRY_RUN:-}"

if [ -z "$DRY_RUN" ]; then
    : "${GITHUB_TOKEN:?GITHUB_TOKEN required}"
    : "${GITHUB_REPOSITORY:?GITHUB_REPOSITORY required}"
fi

log() { printf 'cross-loop: %s\n' "$*"; }

# ── 1. Resolve companion HEAD ────────────────────────────────────────
if [ -n "${GITHUB_TOKEN:-}" ]; then
    git config --global url."https://oauth2:${GITHUB_TOKEN}@agency.lab:3000/".insteadOf "https://agency.lab:3000/"
fi

HEAD_SHA="$(git ls-remote "${COMPANION_URL_BASE}/${COMPANION_REPO}.git" "refs/heads/${BASE_BRANCH}" | awk '{print $1}')"
if ! printf '%s' "$HEAD_SHA" | grep -Eq '^[0-9a-f]{40}$'; then
    log "FATAL: could not resolve ${COMPANION_REPO} ${BASE_BRANCH} HEAD"
    exit 1
fi
log "testing ${COMPANION_REPO}@${BASE_BRANCH} HEAD: ${HEAD_SHA}"

# ── 2. Run cross-dogfood against HEAD ────────────────────────────────
LOGFILE="$(mktemp)"
set +e
COMPANION_SHA="$HEAD_SHA" bash "$SCRIPT_DIR/cross-dogfood.sh" 2>&1 | tee "$LOGFILE"
DOGFOOD_EXIT="${PIPESTATUS[0]}"
set -e
log "cross-dogfood.sh exit=${DOGFOOD_EXIT}"

if [ "$DOGFOOD_EXIT" -eq 0 ]; then
    log "closed-loop clean — no drift at companion HEAD"
    exit 0
fi

# ── 3. Open cross-drift-YYYY-WW issue ────────────────────────────────
WEEK="$(date -u +%Y-%V)"
ISSUE_TITLE="cross-drift-${WEEK}"
LOG_TAIL="$(tail -50 "$LOGFILE")"

body_json="$(python3 - <<'PY' "$COMPANION_REPO" "$HEAD_SHA" "$DOGFOOD_EXIT" "$LOG_TAIL"
import json, sys
companion, sha, code, tail = sys.argv[1:]
body = f"""**Automated cross-drift report** (Tuesday closed-loop cron — see `docs/cross-fixture-bump.md` §6).

- Failing invocation: `COMPANION_SHA={sha} ci/cross-dogfood.sh`
- Companion: `{companion}` @ `{sha}` (develop HEAD, NOT pinned)
- Exit code: `{code}` (see `docs/cross-fixture-bump.md` §1.4)

```
{tail}
```

Per runbook §6, the next PR in this repo is blocked until this issue is resolved. Resolution paths:
1. Fix the finding at companion and bump the pin (runbook §3)
2. Narrow the local rule shape (runbook §5)
3. Pin-hold with open fix PR referenced here (time-boxed)
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
    log "${ISSUE_TITLE} already open — skipping"
    exit 0
fi

payload="$(python3 -c "import json,sys; print(json.dumps({'title':'${ISSUE_TITLE}','body':json.loads(sys.stdin.read())}))" <<< "$body_json")"
curl -sf -X POST -H "Authorization: token ${GITHUB_TOKEN}" \
    -H "Content-Type: application/json" \
    "${API_BASE}/repos/${GITHUB_REPOSITORY}/issues" \
    -d "$payload" >/dev/null
log "opened ${ISSUE_TITLE}"

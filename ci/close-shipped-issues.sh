#!/usr/bin/env bash
# ci/close-shipped-issues.sh
#
# Backfill helper for the bug fixed in #240 — integration PRs that dropped
# `Closes #N` tokens left open forge issues for weeks after their code
# actually shipped. This script surfaces candidates for manual close by
# grepping merged commit history for each supplied open-issue number.
#
# Input: open-issue numbers on stdin, one per line (blank lines + `#`
# comments ignored). Feed from a forge listing:
#
#     tea issues list --state open --output tsv | cut -f1 | \
#         ./ci/close-shipped-issues.sh
#
#     # or from MCP:
#     mcp__forge__forge_list_issues repo=yg/cfdb state=open | \
#         jq -r '.[].number' | \
#         ./ci/close-shipped-issues.sh
#
# Output (stdout): one "CANDIDATE #N:" block per issue whose number
# appears in any commit on origin/main or origin/develop. Each block
# lists the matching commits (short hash + subject) indented two spaces.
#
# Exit 0 always. This script does not close issues. A reviewer reads each
# candidate's commit list, confirms the scope matches the issue body, and
# runs `forge_update_issue` to close. The issue body's "slice's target
# files" check (§Scope Part A step 2) is explicitly NOT automated here
# because parsing natural-language scope sections is unreliable; human
# judgment is the correct filter.

set -euo pipefail

BRANCHES="origin/main origin/develop"

usage() {
    cat >&2 <<'EOF'
Usage: ci/close-shipped-issues.sh [--branches "<ref1> <ref2>"]

Reads open-issue numbers from stdin (one per line, `#` comments + blank
lines ignored). For each:
  - Greps merged commits on the named refs (default: origin/main
    origin/develop) for `#<N>` in subject or body.
  - Emits a "CANDIDATE #<N>:" block to stdout listing matching commits.

Exit 0 always. Decision to close is manual.
EOF
}

while [ $# -gt 0 ]; do
    case "$1" in
        --branches)
            if [ $# -lt 2 ]; then
                printf 'error: --branches requires an argument\n' >&2
                usage
                exit 2
            fi
            BRANCHES="$2"
            shift 2
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            printf 'error: unknown argument: %s\n' "$1" >&2
            usage
            exit 2
            ;;
    esac
done

# Verify refs exist. Warn if missing — likely `git fetch origin` is needed.
for ref in $BRANCHES; do
    if ! git rev-parse --verify --quiet "$ref" >/dev/null 2>&1; then
        printf 'warn: ref %s not found locally; run `git fetch origin` first\n' \
               "$ref" >&2
    fi
done

candidates=0
checked=0

while IFS= read -r line; do
    # Strip trailing whitespace / CR (CRLF-safe).
    line="${line%[[:space:]]}"
    line="${line%$'\r'}"
    # Strip leading whitespace.
    line="${line#"${line%%[![:space:]]*}"}"
    # Skip blanks and `#` comments. A bare `#` or `#` followed by a
    # non-digit is a comment; `#123` is an issue-ref shorthand and
    # must fall through.
    case "$line" in
        ''|'#'|'#'[!0-9]*) continue;;
    esac
    # Accept bare digits or `#123` form.
    num="${line#\#}"
    if ! printf '%s' "$num" | grep -qE '^[0-9]+$'; then
        printf 'warn: ignoring non-numeric input line: %s\n' "$line" >&2
        continue
    fi

    checked=$((checked + 1))

    # Extended regex so we can anchor the number with `([^0-9]|$)` to
    # prevent #12 matching #123. `git log --grep` against the union of
    # named refs — matches if the pattern appears in subject or body.
    # shellcheck disable=SC2086
    matches="$(git log $BRANCHES --extended-regexp \
        --grep="(^|[^0-9])#${num}([^0-9]|\$)" \
        --format='%h %s' 2>/dev/null || true)"

    if [ -z "$matches" ]; then
        continue
    fi

    printf 'CANDIDATE #%s:\n' "$num"
    printf '%s\n' "$matches" | sed 's/^/  /'
    printf '\n'
    candidates=$((candidates + 1))
done

printf 'close-shipped-issues: %d checked, %d candidate(s)\n' \
       "$checked" "$candidates" >&2

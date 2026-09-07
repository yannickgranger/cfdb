#!/usr/bin/env bash

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

for ref in $BRANCHES; do
    if ! git rev-parse --verify --quiet "$ref" >/dev/null 2>&1; then
        printf 'warn: ref %s not found locally; run `git fetch origin` first\n' \
               "$ref" >&2
    fi
done

candidates=0
checked=0

while IFS= read -r line; do
    line="${line%[[:space:]]}"
    line="${line%$'\r'}"
    line="${line#"${line%%[![:space:]]*}"}"
    case "$line" in
        ''|'#'|'#'[!0-9]*) continue;;
    esac
    num="${line#\#}"
    if ! printf '%s' "$num" | grep -qE '^[0-9]+$'; then
        printf 'warn: ignoring non-numeric input line: %s\n' "$line" >&2
        continue
    fi

    checked=$((checked + 1))

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

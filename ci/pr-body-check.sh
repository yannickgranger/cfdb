#!/usr/bin/env bash
# ci/pr-body-check.sh
#
# Enforce the PR-body close-directive contract from #240. PRs whose title
# references an issue via `#N` (the gitea / github auto-close convention)
# must include a matching `Closes #N` / `Fixes #N` / `Resolves #N` footer
# in the body, or an explicit `Bundle: #A, #B, #C, ...` directive listing
# each referenced issue.
#
# Rationale: integration PRs like #224 / #226 bundled multiple RFC-037
# slices (#215/#216/#217, #219/#220) but dropped the per-issue close
# tokens, leaving forge issues open for weeks after merge and causing
# parallel agents to re-implement already-shipped work. This script makes
# the close signal explicit and machine-checked.
#
# Inputs (env vars):
#   PR_TITLE — PR title string
#   PR_BODY  — PR body string (may be empty)
#
# Exit codes:
#   0 — no `#N` in title, or every `#N` has a close directive / bundle entry
#   1 — at least one `#N` in title is missing both a close directive AND a
#       bundle-line mention
#   2 — usage / environment error (PR_TITLE unset AND stdin empty)
#
# Usage in CI (Gitea Actions / GitHub Actions):
#   env:
#     PR_TITLE: ${{ github.event.pull_request.title }}
#     PR_BODY:  ${{ github.event.pull_request.body }}
#   run: bash ci/pr-body-check.sh

set -euo pipefail

PR_TITLE="${PR_TITLE:-}"
PR_BODY="${PR_BODY:-}"

# Skip on non-PR contexts (push events, local invocations without env).
# Returning 0 here keeps the step a no-op on push runs; the CI step should
# gate invocation on pull_request events.
if [ -z "$PR_TITLE" ]; then
    echo "pr-body-check: PR_TITLE unset; skipping (not a PR context)" >&2
    exit 0
fi

# Extract unique `#<digits>` tokens from PR_TITLE. `sort -u` dedupes so a
# title that mentions the same issue twice doesn't multi-count.
mapfile -t issue_refs < <(printf '%s\n' "$PR_TITLE" | grep -oE '#[0-9]+' | sort -u)

if [ "${#issue_refs[@]}" -eq 0 ]; then
    echo "pr-body-check: no #N refs in PR title; skipping"
    exit 0
fi

# Extract any `Bundle:` directive lines (case-insensitive). A bundle line
# declares that the PR intentionally closes multiple issues; each listed
# issue counts as "closed" for this check's purposes.
bundle_lines="$(printf '%s\n' "$PR_BODY" | grep -iE '^[[:space:]]*bundle:' || true)"

missing=()
for ref in "${issue_refs[@]}"; do
    num="${ref#\#}"

    # Path A — `Closes #N` / `Fixes #N` / `Resolves #N` anywhere in body.
    # Use ERE with `[^0-9]|$` boundary so `#12` does not match `#123`.
    if printf '%s\n' "$PR_BODY" | \
       grep -qiE "(closes|fixes|resolves)[[:space:]]+#${num}([^0-9]|\$)"; then
        continue
    fi

    # Path B — `Bundle:` directive that lists `#N` on the same line.
    if [ -n "$bundle_lines" ] && \
       printf '%s\n' "$bundle_lines" | grep -qE "(^|[^0-9])#${num}([^0-9]|\$)"; then
        continue
    fi

    missing+=("$ref")
done

if [ "${#missing[@]}" -gt 0 ]; then
    cat >&2 <<EOF
pr-body-check: FAIL

PR title references the following issue(s) but the PR body has neither a
"Closes #N" footer nor a "Bundle:" directive for them:

  ${missing[*]}

Fix — add one of:

  Closes #<N>            (or Fixes #<N> / Resolves #<N>)

for every referenced issue, OR a single:

  Bundle: #<A>, #<B>, #<C>

line that lists each bundled issue.

See #240 for rationale. This check exists because bundle PRs that drop
close tokens leave forge issues open for weeks post-merge, causing
parallel agents to re-implement shipped work.
EOF
    exit 1
fi

echo "pr-body-check: all ${#issue_refs[@]} #N ref(s) in title have close directives or bundle entries"

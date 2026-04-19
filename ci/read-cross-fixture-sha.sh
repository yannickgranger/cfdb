#!/usr/bin/env bash
# ci/read-cross-fixture-sha.sh
#
# RFC-033 §3.1 / §3.2 — single parse source for `.cfdb/cross-fixture.toml`.
# Echoes the pinned companion SHA to stdout. Used by:
#   - PR-time `ci/cross-dogfood.sh` (Issue A2)
#   - Weekly bump cron (Issue C1)
#   - Weekly closed-loop housekeeping cron (Issue E1)
#
# Centralising the parse (SOLID RC1 — CCP fix) prevents the pattern from
# drifting across three invocation sites and silently accepting the wrong
# field when the TOML layout evolves.
#
# Parse discipline (SOLID RC3):
#   - Anchored at start-of-line to reject commented-out `# sha = "…"`.
#   - `=` explicitly matched so future fields like `sha_prev` cannot match.
#   - First match only (`head -1`) — defensive against duplicate fields.
#   - 40-char hex validation — rejects placeholder zeros at the sentinel
#     value used by brand-new fixtures that have never been bumped.
#
# Usage:
#   ci/read-cross-fixture-sha.sh [FIXTURE_PATH]
#
# FIXTURE_PATH defaults to `.cfdb/cross-fixture.toml` relative to $PWD.
# Pass an explicit path for unit tests that validate against synthetic
# fixtures (see ci/read-cross-fixture-sha-test.sh).
#
# Exit codes:
#   0 — pinned SHA echoed on stdout.
#   1 — fixture file missing.
#   2 — no `sha = "…"` line found (unanchored comment-only or empty file).
#   3 — SHA value is not 40 lowercase hex chars (e.g. 40 zeros sentinel).

set -euo pipefail

FIXTURE_PATH="${1:-.cfdb/cross-fixture.toml}"

if [ ! -f "$FIXTURE_PATH" ]; then
    echo "read-cross-fixture-sha: fixture not found: $FIXTURE_PATH" >&2
    exit 1
fi

# Anchored grep per RFC-033 §3.1 — `^\s*sha\s*=` matches only the field
# assignment, never a commented-out sample or a future `sha_prev` twin.
sha_line="$(grep -E '^\s*sha\s*=' "$FIXTURE_PATH" | head -1 || true)"

if [ -z "$sha_line" ]; then
    echo "read-cross-fixture-sha: no sha field in $FIXTURE_PATH" >&2
    exit 2
fi

# Extract the value between the first pair of double quotes on that line.
sha_value="$(printf '%s\n' "$sha_line" | cut -d'"' -f2)"

# Validate: 40 lowercase hex chars, not the all-zeros placeholder.
if ! printf '%s' "$sha_value" | grep -Eq '^[0-9a-f]{40}$'; then
    echo "read-cross-fixture-sha: sha is not 40 lowercase hex chars: '$sha_value'" >&2
    exit 3
fi

if [ "$sha_value" = "0000000000000000000000000000000000000000" ]; then
    echo "read-cross-fixture-sha: sha is the uninitialised placeholder" >&2
    exit 3
fi

printf '%s\n' "$sha_value"

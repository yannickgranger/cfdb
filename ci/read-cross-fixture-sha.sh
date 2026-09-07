#!/usr/bin/env bash

set -euo pipefail

FIXTURE_PATH="${1:-.cfdb/cross-fixture.toml}"

if [ ! -f "$FIXTURE_PATH" ]; then
    echo "read-cross-fixture-sha: fixture not found: $FIXTURE_PATH" >&2
    exit 1
fi

sha_line="$(grep -E '^\s*sha\s*=' "$FIXTURE_PATH" | head -1 || true)"

if [ -z "$sha_line" ]; then
    echo "read-cross-fixture-sha: no sha field in $FIXTURE_PATH" >&2
    exit 2
fi

sha_value="$(printf '%s\n' "$sha_line" | cut -d'"' -f2)"

if ! printf '%s' "$sha_value" | grep -Eq '^[0-9a-f]{40}$'; then
    echo "read-cross-fixture-sha: sha is not 40 lowercase hex chars: '$sha_value'" >&2
    exit 3
fi

if [ "$sha_value" = "0000000000000000000000000000000000000000" ]; then
    echo "read-cross-fixture-sha: sha is the uninitialised placeholder" >&2
    exit 3
fi

printf '%s\n' "$sha_value"

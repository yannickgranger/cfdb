#!/usr/bin/env bash
# ci/cross-dogfood-binaries-test.sh
#
# Unit tests for the binary-presence refusals in ci/cross-dogfood.sh.
#
# RFC-039 §7.2 orders the self-enrich-deprecation pass to run against the
# companion at the pinned SHA; RFC-033 §3.6 has the closed-loop job assert
# zero violations. A run that reports a pass with that pass unbuilt reports
# a question it never asked, so an absent binary refuses (exit 2) instead.
#
# Follows the ci/read-cross-fixture-sha-test.sh convention — plain
# assertions, no framework. Exits non-zero on any failure.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SCRIPT="$SCRIPT_DIR/cross-dogfood.sh"

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

fail=0
pass=0

present_cfdb="$TMP/cfdb"
present_enrich="$TMP/dogfood-enrich"
printf '#!/bin/sh\nexit 0\n' >"$present_cfdb"
printf '#!/bin/sh\nexit 0\n' >"$present_enrich"
chmod +x "$present_cfdb" "$present_enrich"

assert_refusal() {
    local name="$1" cfdb_bin="$2" enrich_bin="$3" want_exit="$4" want_text="$5"
    local out got_exit=0
    out="$(CFDB_BIN="$cfdb_bin" DOGFOOD_BIN="$enrich_bin" \
        COMPANION_DIR="$TMP/companion" bash "$SCRIPT" 2>&1)" || got_exit=$?
    if [ "$got_exit" -ne "$want_exit" ]; then
        fail=$((fail + 1))
        echo "FAIL: $name — expected exit $want_exit, got $got_exit" >&2
        return
    fi
    case "$out" in
    *"$want_text"*)
        pass=$((pass + 1))
        echo "PASS: $name (exit $got_exit)"
        ;;
    *)
        fail=$((fail + 1))
        echo "FAIL: $name — exit $got_exit but no line naming '$want_text'" >&2
        ;;
    esac
}

assert_reaches_the_clone() {
    local name="$1" out got_exit=0
    out="$(CFDB_BIN="$present_cfdb" DOGFOOD_BIN="$present_enrich" \
        COMPANION_REPO="yg/does-not-exist-$$" \
        COMPANION_DIR="$TMP/companion-control" bash "$SCRIPT" 2>&1)" || got_exit=$?
    if [ "$got_exit" -eq 2 ]; then
        fail=$((fail + 1))
        echo "FAIL: $name — both binaries present yet the script still refused with exit 2" >&2
        return
    fi
    case "$out" in
    *"binary not found"*)
        fail=$((fail + 1))
        echo "FAIL: $name — both binaries present yet a not-found line was printed" >&2
        ;;
    *)
        pass=$((pass + 1))
        echo "PASS: $name (got past the presence checks, exit $got_exit)"
        ;;
    esac
}

assert_refusal "absent dogfood-enrich refuses" \
    "$present_cfdb" "$TMP/no-such-dogfood-enrich" 2 \
    "dogfood-enrich binary not found"

assert_refusal "absent dogfood-enrich names why a pass would be wrong" \
    "$present_cfdb" "$TMP/no-such-dogfood-enrich" 2 \
    "report a pass it never measured"

assert_refusal "absent cfdb refuses" \
    "$TMP/no-such-cfdb" "$present_enrich" 2 \
    "cfdb binary not found"

assert_reaches_the_clone "both binaries present clears the presence checks"

echo "cross-dogfood-binaries-test: $pass passed, $fail failed"
[ "$fail" -eq 0 ]

#!/usr/bin/env bash
# ci/cross-dogfood.sh (cfdb flavour)
#
# RFC-033 §3.2 — runs the locally-built cfdb binary against the companion
# repo (graph-specs-rust) at the SHA pinned in `.cfdb/cross-fixture.toml`.
# Scripted once, invoked by:
#   - PR-time CI step (`.gitea/workflows/ci.yml`)
#   - Weekly cross-bump cron (Issue #67, Monday 06:00 UTC)
#   - Weekly closed-loop cron at companion HEAD (Issue #70, Tuesday 06:00 UTC)
#
# Differentiated exit codes (rust-systems B2 — for diagnosis without
# eyeballing logs):
#   0  — cross-dogfood pass (all ban rules zero rows on companion)
#   10 — companion clone or checkout failed (infra problem, not a drift)
#   20 — `cfdb extract` failed on the companion tree — most often a
#        SchemaVersion mismatch during an I2 lockstep window (see
#        RFC-033 §3.3 / Invariant I2). Fix: finish the lockstep bump
#        on graph-specs before expecting this job to pass.
#   30 — at least one `cfdb violations --rule` returned non-zero rows
#        on the companion tree. Genuine finding. Per RFC-033 §3.4 there
#        is NO allowlist: either fix the finding in the companion repo
#        (land a fix PR there, then bump .cfdb/cross-fixture.toml to
#        consume the new SHA), or scope the rule narrower — never add
#        an exemption file.
#
# Two SHA universes (rust-systems C1). The cfdb binary used here is THIS
# PR's `./target/release/cfdb`, NOT the pinned-SHA cfdb installed from
# `.cfdb/cfdb.rev` by graph-specs' own `cfdb-check` job. The separation is
# intentional and load-bearing:
#   - THIS script answers "does my current cfdb handle the companion's
#     code?" (cross-dogfood test target is the companion SOURCE TREE).
#   - Graph-specs' cfdb-check answers "does the companion's code satisfy
#     a known-good cfdb's ban rules?" (cross-dogfood test target is this
#     repo's OWN code, via a pinned companion binary).
# Future maintainers: do NOT "fix" the divergence by unifying the SHAs.
# The questions differ.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

COMPANION_REPO="${COMPANION_REPO:-yg/graph-specs-rust}"
COMPANION_URL_BASE="${COMPANION_URL_BASE:-https://agency.lab:3000}"
COMPANION_DIR="${COMPANION_DIR:-$(mktemp -d)}"
CFDB_BIN="${CFDB_BIN:-$REPO_ROOT/target/release/cfdb}"

if [ ! -x "$CFDB_BIN" ]; then
    echo "cross-dogfood: cfdb binary not found at $CFDB_BIN" >&2
    echo "  hint: cargo build -p cfdb-cli --release --bin cfdb" >&2
    exit 2
fi

# The pinned SHA is the default; the weekly bump cron (Issue #67,
# ci/cross-bump.sh) overrides via COMPANION_SHA env to test against
# companion develop HEAD. Same script, two universes: PR-time uses the
# pin for reproducibility; the cron uses HEAD to detect pin staleness.
COMPANION_SHA="${COMPANION_SHA:-$("$SCRIPT_DIR/read-cross-fixture-sha.sh")}"

# Clone + checkout the pinned companion SHA. Use --filter=blob:none to
# avoid pulling the whole history; we only need the tree at this SHA.
if [ -n "${GITHUB_TOKEN:-}" ]; then
    git config --global url."https://oauth2:${GITHUB_TOKEN}@agency.lab:3000/".insteadOf "https://agency.lab:3000/"
fi
git clone --filter=blob:none "${COMPANION_URL_BASE}/${COMPANION_REPO}.git" "$COMPANION_DIR" \
    || exit 10
(cd "$COMPANION_DIR" && git checkout "$COMPANION_SHA") || exit 10

# Namespaced keyspace (clean-arch CA-3 — prevents SHA-scoped collisions
# with the self-audit `cfdb-self` keyspace that shares `.cfdb/db/` per
# rust-systems C2 dual-keyspace accumulation).
KEYSPACE="cross-companion-${COMPANION_SHA:0:12}"
DB_DIR="${CFDB_DB_DIR:-$REPO_ROOT/.cfdb/db}"
mkdir -p "$DB_DIR"

"$CFDB_BIN" extract \
    --workspace "$COMPANION_DIR" \
    --db "$DB_DIR" \
    --keyspace "$KEYSPACE" >/dev/null \
    || exit 20

# Iterate every arch-ban-*.cypher. --count-only --no-fail combined lets
# the script capture the count and tally findings without the first
# non-zero rule tripping `set -e`.
found=0
for rule in "$REPO_ROOT"/examples/queries/arch-ban-*.cypher; do
    rows="$("$CFDB_BIN" violations \
        --db "$DB_DIR" \
        --keyspace "$KEYSPACE" \
        --rule "$rule" \
        --count-only \
        --no-fail)"
    if [ "$rows" -gt 0 ]; then
        echo "cross-dogfood: $(basename "$rule") returned $rows rows on ${COMPANION_REPO}@${COMPANION_SHA:0:12}"
        found=$((found + rows))
    fi
done

# RFC-039 §7.2 (#343) — also run the self-enrich-deprecation dogfood
# against the companion source tree at the pinned SHA. The cfdb /
# graph-specs duo (RFC-033) means an extractor-side recall regression
# on `#[deprecated]` would silently invalidate every downstream
# graph-specs verdict on companion code; the cross-pass catches it
# at PR time.
#
# The harness is skipped if its binary is absent (older CI configs
# that haven't shipped the dogfood-enrich step yet) — the script's
# default exit semantics still gate on arch-ban rule rows.
DOGFOOD_BIN="${DOGFOOD_BIN:-$REPO_ROOT/target/release/dogfood-enrich}"
if [ -x "$DOGFOOD_BIN" ]; then
    echo "cross-dogfood: running self-enrich-deprecation against ${COMPANION_REPO}@${COMPANION_SHA:0:12}"
    rc=0
    "$DOGFOOD_BIN" \
        --pass enrich-deprecation \
        --db "$DB_DIR" \
        --keyspace "$KEYSPACE" \
        --cfdb-bin "$CFDB_BIN" \
        --workspace "$COMPANION_DIR" \
        || rc=$?
    case "$rc" in
        0)
            echo "cross-dogfood: self-enrich-deprecation 0 violations on companion"
            ;;
        30)
            # Per RFC-033 §3.4 + RFC-039 §7.2 Cross-dogfood row: any
            # row blocks merge. Tally into `found` so the final exit
            # surfaces a unified count.
            echo "cross-dogfood: self-enrich-deprecation FAIL on ${COMPANION_REPO}@${COMPANION_SHA:0:12}" >&2
            found=$((found + 1))
            ;;
        *)
            # Runtime error (exit 1) — surfaces as exit 20 to match
            # the `cfdb extract` semantics: harness configuration
            # problem, not a verdict.
            echo "cross-dogfood: self-enrich-deprecation runtime error (exit $rc)" >&2
            exit 20
            ;;
    esac
else
    echo "cross-dogfood: dogfood-enrich binary not found at $DOGFOOD_BIN — skipping self-enrich-deprecation pass (build it via `cargo build -p dogfood-enrich --release` to enable)"
fi

if [ "$found" -eq 0 ]; then
    echo "cross-dogfood: 0 violations on ${COMPANION_REPO}@${COMPANION_SHA:0:12}"
    exit 0
fi

echo "cross-dogfood: FAIL — $found total violations across arch-ban-*.cypher + self-enrich-deprecation" >&2
exit 30

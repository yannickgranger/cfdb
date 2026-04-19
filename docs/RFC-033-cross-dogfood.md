---
title: RFC-033 — Cross-dogfood discipline with graph-specs-rust
status: Ratified
date: 2026-04-19
authors: cross-dogfood-review team (team-lead drafted; clean-arch, ddd, solid, rust-systems ratified)
companion: yg/graph-specs-rust RFC-002 (same topic, mirror)
---

# RFC-033 — Cross-dogfood discipline with graph-specs-rust

## §1 — Problem

cfdb and graph-specs-rust are a paired toolchain. cfdb is the **X-ray** (detect existing debt in a Rust workspace); graph-specs is the **vaccine** (block new drift at PR time). Both are being built iteratively to support a concrete rescue mission on `yg/qbot-core`.

The failure mode this RFC prevents: **"rescue tools that work on a synthetic fixture but drift from what qbot-core actually presents."** Concretely:

1. cfdb adds a new fact type, but graph-specs (which vendors cfdb as a pinned git dep) doesn't know how to consume it. Discovered when a qbot-core PR breaks.
2. graph-specs adds a new equivalence level, but cfdb's self-audit flags it as a false positive on cfdb's own tree. Discovered when cfdb CI flaps.
3. A new classifier Cypher rule (Phase 3, RFC-032 §4) flags findings on cfdb or graph-specs themselves. Shipped anyway because nobody ran it on the author-repos first — then it carpets qbot-core with false positives and the rescue loses credibility.
4. cfdb bumps `SchemaVersion`; downstream graph-specs CI misses the window because the bump wasn't coordinated.

The symmetric truth: both tools ARE Rust workspaces. Both have public surfaces, bounded contexts (however minimal), and the same patterns they're designed to detect in qbot-core. If the tools can't verify they work cleanly on their own authors' trees, they can't be trusted against qbot-core at rescue time.

Today each tool dogfoods itself (cfdb's CI runs `cfdb extract` on cfdb's own tree; graph-specs' CI runs `graph-specs check` on its own specs + code). The missing piece is **cross-dogfood**: each tool runs against the sibling tool's tree on every PR.

## §2 — Scope

In scope:

1. A `.cross-fixture.toml` file in both repos that pins the sibling repo at a known SHA.
2. A CI step in each repo that clones the sibling at the pinned SHA and runs the local tool against the fixture's tree.
3. A bump protocol for advancing the pinned SHAs (weekly scheduled, manual on-demand).
4. A schema-version coordination rule: cfdb's `SchemaVersion` bump PR must include the matching graph-specs fixture bump in the same atomic lockstep.
5. A "zero-false-positive on siblings" invariant: every new cfdb Cypher rule and every new graph-specs equivalence level must produce zero findings against BOTH repos' own trees before it ships.
6. Extension to the `Tests:` prescription from CLAUDE.md §2.5: every new-capability issue now requires a cross-dogfood assertion as the second test entry (after unit, before qbot-core target).
7. A weekly closed-loop housekeeping job (one scheduled CI run per week) that cross-dogfoods at HEAD (not pinned) and opens an issue if either repo has drifted against the other's develop tip.

Out of scope (explicit non-goals in §6):

- Publishing cfdb or graph-specs to crates.io (path dep / pinned git dep model stands).
- Requiring qbot-core-SHA pins in cfdb CI (qbot-core is the rescue target, not a rescue tool; it's pinned only per-rescue-PR as the `Tests:` target, not workspace-wide).
- Bidirectional schema invariants beyond `cfdb::SchemaVersion` (graph-specs does not emit a schema versioning cfdb depends on).

## §3 — Design

### §3.1 — `.cfdb/cross-fixture.toml`

Single file at `.cfdb/cross-fixture.toml` in each repo. This honours the RFC-030 §4 registry boundary (the repo root is reserved for `docs/` rationale and `specs/` contracts; infrastructure config lives under `.cfdb/` alongside `.cfdb/queries/` and `.cfdb/db/`).

Vocabulary note: "companion repo" is used throughout this RFC to mean the paired tool's repository (cfdb ↔ graph-specs-rust). "sibling" is reserved for RFC-001-style DDD context-sibling relationships inside a single repo — do not cross the two terms.

```toml
# .cfdb/cross-fixture.toml — cross-dogfood fixture pin per RFC-033.
# Bumped by the weekly cross-bump job (CI scheduled) or manually in an
# RFC-033-gated PR. The pinned SHA is expected to be clean on both tools.

[companion]
# For cfdb: the graph-specs-rust commit to test against.
# For graph-specs-rust: the cfdb commit to test against.
repo      = "yg/graph-specs-rust"        # or "yg/cfdb" in the companion
branch    = "develop"                     # documentation only; SHA is authoritative
sha       = "0000000000000000000000000000000000000000"
bumped_at = "2026-04-19T00:00:00Z"
bumped_by = "initial"
```

**Parse discipline (SOLID RC3):** CI parses with an anchored-and-equals-anchored grep to prevent false matches on future TOML comments:

```bash
# SAFE: matches `sha = "…"` only, rejects `# sha = "…"`.
grep -E '^\s*sha\s*=' .cfdb/cross-fixture.toml | head -1 | cut -d'"' -f2
```

No TOML crate dependency on the CI step. The parse is centralised in a shared helper (§3.2).

### §3.2 — CI cross-dogfood step (via shared helper)

Per CCP (solid RC1) and composition-root clarity (clean-arch CA-2), the cross-dogfood shell logic is extracted into two shared helpers committed under `ci/`:

- `ci/read-cross-fixture-sha.sh` — parses `.cfdb/cross-fixture.toml` and echoes the pinned companion SHA. Single source of truth for the parse pattern; used by both the PR-time cross-dogfood step AND the weekly bump / closed-loop jobs (§3.3, §3.6).
- `ci/cross-dogfood.sh` — clones the companion repo at the pinned SHA into `/tmp/companion` and runs the local tool against it. Exit codes are differentiated (see below).

CI YAML becomes a thin dispatch:

```yaml
- name: Cross-dogfood — cfdb on graph-specs
  env:
    GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
  run: |
    cd repo
    ./ci/cross-dogfood.sh
```

The shared `ci/cross-dogfood.sh` (cfdb flavour — graph-specs-rust has a mirror with `graph-specs check` instead of `cfdb extract + violations`):

```bash
#!/usr/bin/env bash
# ci/cross-dogfood.sh — run local tool against the pinned companion repo.
# Exit codes (rust-systems B2 — differentiated for diagnosis):
#   0  = cross-dogfood pass
#   10 = companion clone/checkout failed (infra problem, not a drift)
#   20 = `cfdb extract` failed on companion tree (likely SchemaVersion
#        mismatch during lockstep window; see RFC-033 §3.3)
#   30 = at least one `cfdb violations --rule` returned non-empty rows
#        (genuine finding — either a new ban rule flagged the companion
#        or the companion landed a regression; fix at source or scope
#        the rule per §3.4, never allowlist)
set -euo pipefail

COMPANION_SHA="$(./ci/read-cross-fixture-sha.sh)"
COMPANION_REPO="yg/graph-specs-rust"
COMPANION_DIR="/tmp/companion"

# Clone companion at pinned SHA.
git config --global url."https://oauth2:${GITHUB_TOKEN}@agency.lab:3000/".insteadOf "https://agency.lab:3000/"
git clone "https://agency.lab:3000/${COMPANION_REPO}.git" "$COMPANION_DIR" || exit 10
(cd "$COMPANION_DIR" && git checkout "$COMPANION_SHA") || exit 10

# Namespaced keyspace (clean-arch CA-3 — prevents SHA-scoped collisions with
# the self-audit `cfdb-self` keyspace that also lives in .cfdb/db/ per
# intentional dual-keyspace accumulation, rust-systems C2).
KEYSPACE="cross-companion-${COMPANION_SHA:0:12}"

# The cfdb binary in use here is THIS PR's build (./target/release/cfdb),
# NOT the pinned-SHA binary installed by graph-specs' cfdb-check CI job.
# Intentional decoupling (rust-systems C1): this run answers "does my
# current cfdb handle the companion's code?"; graph-specs' cfdb-check
# answers "does the companion's code satisfy a known-good cfdb's rules?"
# Do not "fix" the divergence by unifying the SHAs — the questions differ.
./target/release/cfdb extract --workspace "$COMPANION_DIR" --db .cfdb/db --keyspace "$KEYSPACE" || exit 20

found=0
for rule in examples/queries/arch-ban-*.cypher; do
    rows="$(./target/release/cfdb violations --db .cfdb/db --keyspace "$KEYSPACE" --rule "$rule" --count-only)"
    if [ "$rows" -gt 0 ]; then
        echo "cross-dogfood: $rule returned $rows rows on $COMPANION_REPO@${COMPANION_SHA:0:12}"
        found=$((found + 1))
    fi
done
[ "$found" -eq 0 ] || exit 30
```

The `--count-only` flag on `cfdb violations` is a small addition this RFC assumes — if `cfdb violations` does not currently emit a terse row count, Issue A2 includes adding `--count-only` or parsing stdout. That scope is tracked in §7.

### §3.3 — Bump protocol

**Weekly automatic bump** (distinct cron per repo to avoid collision, rust-systems C3):

- cfdb — **Monday 06:00 UTC**
- graph-specs-rust — **Monday 06:30 UTC**

Each scheduled workflow:

1. Clones the companion at current `develop` HEAD.
2. Runs the local tool against it via `ci/cross-dogfood.sh`.
3. If exit 0: opens a PR that updates `.cfdb/cross-fixture.toml` with the new SHA.
4. If non-zero: opens a `cross-drift-YYYY-WW` issue with the failing invocation, companion SHA, and exit code (10 / 20 / 30 per §3.2).

**Manual bump:** any contributor can open a PR bumping the pin when they know the sibling has landed a change they need to consume. The PR must include:

- The new SHA.
- A one-line rationale (e.g. "picks up new `:Visibility` fact kind from cfdb #35").
- CI must pass on the bump PR just like any other.

**Schema-version lockstep:** cfdb PRs that bump `cfdb_core::SchemaVersion` MUST also include a draft PR on graph-specs-rust bumping `.cross-fixture.toml` to the cfdb PR's HEAD SHA. Neither side merges alone — they merge in order (cfdb first, then graph-specs fixture bump within minutes). The lockstep is author discipline, not a mechanical CI gate; violating it triggers a `cross-drift` issue immediately after the next weekly bump job.

### §3.4 — Zero-false-positive invariant

**This is a named obligation on every Cypher-rule author and every equivalence-level contributor** (SOLID RC2). The invariant is:

> A new `.cypher` rule or a new equivalence-level activation MUST produce zero findings against the companion repo at the currently-pinned `.cfdb/cross-fixture.toml` SHA. The PR shipping the new rule/level includes a CI run of `ci/cross-dogfood.sh` as part of its acceptance, same as the `Tests:` prescription mandates (§3.5 / CLAUDE.md §2.5).

This is not an implicit CI behaviour — it is a contract. Issue A2 (§7 decomposition) names this obligation explicitly and every new-rule/new-level issue derived from RFC-032 / RFC-002 §5.2 must carry a `Tests: Cross dogfood` line asserting zero findings on the companion.

Enforcement path at CI time: `ci/cross-dogfood.sh` exits with code 30 on any non-empty rule match. The shipping PR is blocked.

Escape hatch is **intentionally narrow**: the violating PR must either (a) fix the finding in the companion repo in a coordinated prior PR and bump `.cfdb/cross-fixture.toml` to consume the fix, or (b) scope the rule more narrowly to exclude the false-positive shape. There is no allowlist, no ignore-file, no exemption. This is consistent with the global no-metric-ratchets rule (CLAUDE.md §6 / `~/.claude/CLAUDE.md §6 rule 8`).

If a rule is genuinely correct and the companion is the one with real debt, the fix is to file an issue on the companion and land a fix there FIRST, then bump the fixture SHA to consume it.

### §3.5 — `Tests:` prescription extension

CLAUDE.md §2.5 (added in PR #63 / #21) mandates architects prescribe tests per issue. This RFC extends the prescription template:

```
Tests:
  - Unit: <pure-function assertions>
  - Self dogfood (cfdb on cfdb OR graph-specs on graph-specs): <assertion shape>
  - Cross dogfood (cfdb on graph-specs OR graph-specs on cfdb): <assertion shape>
  - Target dogfood (on qbot-core at pinned SHA): <assertion shape; often
    "reports metric X in PR body for reviewer sanity-check">
```

The "Cross dogfood" line is new. An architect CAN prescribe it as `Cross dogfood: none — rationale: <why>` but only with an explicit reason (e.g. "this change is internal to cfdb's persist layer and cannot flow through graph-specs' reader"). The default is an explicit assertion.

### §3.6 — Weekly closed-loop housekeeping

Separate from the pin-bump jobs (§3.3), each repo runs a closed-loop job at **distinct cron times to prevent issue-tracker noise collision** (rust-systems C3):

- cfdb closed-loop — **Tuesday 06:00 UTC** (24h after the cfdb bump job on Monday)
- graph-specs-rust closed-loop — **Tuesday 06:30 UTC**

Each job runs `ci/cross-dogfood.sh` against HEAD of the companion repo (NOT the pinned SHA):

1. cfdb clones graph-specs develop HEAD, runs cfdb tool, asserts zero violations.
2. graph-specs-rust clones cfdb develop HEAD, runs graph-specs check, asserts zero violations.

Any failure opens an issue `cross-drift-YYYY-WW` in the failing repo with the failing invocation, full output, the companion's HEAD SHA, and the `ci/cross-dogfood.sh` exit code (10/20/30 per §3.2). Neither repo's next PR merges until the drift issue is resolved.

This job catches the window between "companion landed a change" and "our next manual pin bump" — if a companion change would break us at HEAD, we know within a week, not at rescue time.

**Shared keyspace accumulation** (rust-systems C2): both the PR-time self-audit (keyspace `cfdb-self`) and the cross-dogfood step (keyspace `cross-companion-<sha12>`) write to the same `.cfdb/db/` directory. This is intentional — keyspaces are isolated by name in the persisted graph, and the determinism check (`ci/determinism-check.sh`) only validates the `cfdb-self` keyspace shape. The cross-dogfood keyspace is not determinism-checked (different workspace, different SHA), which is correct.

## §4 — Invariants

Every change under this RFC must preserve:

- **I1 — Determinism.** Cross-dogfood invocations must be byte-stable given the same pinned SHA + toolchain. No wall-clock or randomised output in the tool's canonical dump; this is the existing cfdb G1 determinism guarantee extended across repos.
- **I2 — Schema-version monotonic.** `cfdb_core::SchemaVersion` bumps happen in cfdb first; graph-specs' fixture bump follows in minutes, not hours. If a cfdb PR proposes a bump and the cross-dogfood CI step can't find a matching graph-specs fixture PR within the review window, the cfdb PR is not merged.
- **I3 — Recall doesn't regress.** cfdb-recall against its own `cfdb-core` stays ≥ 95% across cross-dogfood evolution. Adding a new fact kind for qbot-core rescue must not drop recall on cfdb-core.
- **I4 — No allowlist.** Zero-false-positive invariant (§3.4) has no escape hatch file. The violating PR either fixes, scopes, or doesn't merge.
- **I5 — Keyspace backward-compat.** When cfdb's SchemaVersion bumps, cross-dogfood CI verifies graph-specs can still read the OLD fixture SHA's keyspace shape until the bump PR merges; post-merge, both sides move to the new shape in lockstep.

## §5 — Architect lenses

Dedicated subsections per architect perspective. Verdicts captured inline after review.

### §5.1 — Clean architecture (`clean-arch`)

Open question resolved: `.cfdb/cross-fixture.toml` (§3.1). Rationale: RFC-030 §4 registry boundary reserves repo root for `docs/` + `specs/`; infrastructure pin files live in `.cfdb/` alongside `.cfdb/queries/` and `.cfdb/db/`.

Open question resolved: cross-dogfood step lives in `.gitea/workflows/ci.yml` but delegates all shell to `ci/cross-dogfood.sh` + `ci/read-cross-fixture-sha.sh`. The composition concern is the shell extraction, not which YAML file the step lives in.

**Verdict (round 2, 2026-04-19): RATIFY.** All two blockers (CA-1, CA-3) and four non-blockers (CA-2, CA-4, vocabulary, forward-compat) resolved with file:line citations against revision 1 (commit `2fa9e22`). Dependency rule clean: CI runs tool against companion repo, no reverse import, no inner-layer coupling. Port purity preserved — cross-dogfood invokes the CLI binary (published surface), not `application/`'s lib internals. Zero-false-positive invariant with no-allowlist escape hatch consistent with RATIFIED.md §A.9. Composition root explicit (`ci/cross-dogfood.sh`). Keyspace naming SHA-namespaced and determinism-safe.

### §5.2 — DDD (`ddd-specialist`)

Open question resolved: two bounded contexts with a shared kernel (cfdb's NDJSON/Cypher output format). Vocabulary disambiguated — "companion repo" for the cross-tool relationship, "sibling" reserved for RFC-001-style DDD sibling-context vocabulary inside a single repo.

Open question resolved: `:Finding` (cfdb, persistent classifier node with git-history signals) and `Violation` (graph-specs, ephemeral per-run diff output) are semantically distinct on temporal and subject axes. The RFC correctly does not unify them; unification would force cfdb to know about spec files and graph-specs to know about git history.

**Verdict (round 1, 2026-04-19): RATIFY** with three recorded non-blocking concerns (H1 context-vocabulary qualification, C2 emerging third-context ownership, C3 dependency-direction precision). All three addressed in revision 1 (§3.1 vocabulary notes, §6 item 4, §6 item 3; Issue C2 names the runbook as canonical orchestration-vocabulary home).

### §5.3 — SOLID (`solid-architect`)

Open question resolved: stable grep pattern `^\s*sha\s*=` (robust against future TOML comment additions or field reordering). No TOML crate dependency at the CI step. Parse centralised in `ci/read-cross-fixture-sha.sh` (SOLID RC1 — CCP fix; single parse source used by PR-time, bump, and closed-loop jobs).

Open question resolved: bump-protocol job cohesion is acceptable as one job — the three sub-responsibilities (clone, test, open-PR-or-issue) all change for the same reason ("weekly companion-SHA maintenance"). CCP satisfied.

**Verdict (round 1, 2026-04-19): RATIFY conditional.** All three required changes (RC1 shared parser, RC2 zero-false-positive invariant named as explicit author obligation, RC3 stable grep) resolved in revision 1. Component-metrics impact: Zone of Pain scores for cfdb-core and graph-specs-rust's domain crate are unchanged (RFC-033 adds no Rust crate dependencies); ISP/CRP/SDP directions all satisfied; ADP no cycle at Rust-crate level (SHA pin cycle is human-mediated deployment protocol, not a compile-time dependency edge).

### §5.4 — Rust systems (`rust-systems`)

Open question resolved: cross-dogfood overhead is ~20–30s on cfdb side (clone + syn extract). Binary caching (`/cache/cargo/bin/<tool>-<sha>`) is NOT needed; sccache warmth is sufficient. On graph-specs side, sccache must be added to graph-specs CI as part of Issue B2 (previously absent — overdue independent of this RFC).

Open question resolved: the two SHA universes (Mechanism A `cargo install --branch develop` for CI self-gates vs Mechanism B pinned-SHA in `.cfdb/cross-fixture.toml` for cross-dogfood test target) are intentionally decoupled. They answer different questions: A is "does my tool work against its own tree?"; B is "does my current tool handle the companion's code?". The `ci/cross-dogfood.sh` script carries a multi-line comment (§3.2) explaining this so future maintainers do not "fix" the divergence by unifying the SHAs.

**Verdict (round 2, 2026-04-19): RATIFY.** Both blockers (B1 sccache gap, B2 failure-mode differentiation) and three mandatory prose additions (C1 SHA-universe clarifier, C2 dual-keyspace note, C3 distinct cron schedules) resolved in revision 1. RFC-032's four sequencing traps are orthogonal to this RFC. `cfdb-recall::runner` feature is irrelevant. No circular SHA contamination. Workspace Cargo.toml impact: zero new Rust dependencies.

## §6 — Non-goals

1. Not publishing either tool to crates.io. The pinned-git-dep + cross-fixture model is the release mechanism for this paired toolchain.
2. Not pinning a qbot-core SHA workspace-wide in cfdb CI. qbot-core is the rescue target, not a rescue tool. Per-rescue-PR `Tests:` prescriptions pin qbot-core SHAs as needed for that PR's assertion.
3. Not introducing bidirectional schema invariants where graph-specs emits a schema that cfdb must consume. The flow is one-directional: cfdb emits facts, graph-specs consumes via `cfdb violations`.
4. Not gating cfdb's develop branch on graph-specs' CI being green (and vice versa). Each repo's CI is authoritative for its own develop; the cross-dogfood is per-PR, not per-branch.
5. Not requiring the weekly closed-loop job to auto-remediate. It opens an issue; humans fix.

## §7 — Issue decomposition

One vertical slice per issue. Each carries the `Tests:` line prescribed by the architect team after review.

**Group A — fixture file + CI wiring (cfdb):**

- Issue A1: Add `.cfdb/cross-fixture.toml` to cfdb with initial graph-specs-rust SHA. Schema per §3.1. Add `ci/read-cross-fixture-sha.sh` shared parser (solid RC1).
- Issue A2: Wire cross-dogfood CI step in cfdb's `.gitea/workflows/ci.yml` via `ci/cross-dogfood.sh`. Exit codes 10/20/30 differentiated per §3.2 (rust-systems B2). **Must also add `--count-only` (or equivalent) to `cfdb violations` if it does not emit a terse row count today.** The zero-false-positive invariant obligation (§3.4) is named in the issue body as an author-facing contract for every future rule addition (SOLID RC2).

**Group B — fixture file + CI wiring (graph-specs-rust):**

- Issue B1: Add `.cfdb/cross-fixture.toml` to graph-specs-rust with initial cfdb SHA. Mirror of A1. Add `ci/read-cross-fixture-sha.sh` shared parser.
- Issue B2: Wire cross-dogfood CI step in graph-specs-rust's CI via `ci/cross-dogfood.sh`. **Must include sccache setup** (rust-systems B1) — cfdb clone + build is ~60–120s cold without sccache; mirror the setup step from cfdb's `ci.yml` lines 60–72. Alternative: document cold-run cost as accepted in the issue and move on; RFC recommends the setup-step add since it's overdue independent of this RFC. The cross-dogfood integration test (if one is added beyond the CI step) belongs in `tests/cross_dogfood.rs`, NOT in `application/` (clean-arch CA-4). Zero-false-positive invariant (§3.4) named in issue body.

**Group C — bump protocol:**

- Issue C1: Weekly cron workflow in both repos that attempts a pin bump to the companion's develop HEAD. Cron schedules per §3.3 (cfdb Monday 06:00 UTC, graph-specs-rust Monday 06:30 UTC). Opens PR on success, `cross-drift-YYYY-WW` issue on failure with exit code 10/20/30.
- Issue C2: Author `docs/cross-fixture-bump.md` runbook. **This runbook also declares the cross-dogfood orchestration vocabulary** (ddd C2): `.cfdb/cross-fixture.toml` schema, "pinned SHA", "companion repo", the `cross-drift-YYYY-WW` issue naming convention, and the `ci/cross-dogfood.sh` exit-code contract. The runbook is the canonical home for cross-repo orchestration terminology — neither cfdb's `docs/RFC-*` nor graph-specs' `docs/rfc/*` owns these concepts; the runbook does.

**Group D — schema-version lockstep:**

- Issue D1: Document the `cfdb::SchemaVersion` bump protocol in cfdb's RFC §4 invariants section and reference it from graph-specs-rust's CLAUDE.md.

**Group E — closed-loop housekeeping:**

- Issue E1: Weekly cron workflow in each repo that runs cross-dogfood at companion `develop` HEAD (not pinned). Cron schedules per §3.6 (cfdb Tuesday 06:00 UTC, graph-specs-rust Tuesday 06:30 UTC). Failure opens a `cross-drift-YYYY-WW` issue.

**Group F — `Tests:` prescription template:**

- Issue F1: Extend CLAUDE.md §2.5 in both repos to include the cross-dogfood line in the `Tests:` template.

Each issue's `Tests:` section is prescribed by the architect team during review (§5). Default: unit test for fixture parsing, integration test running the cross-dogfood step on a fresh checkout, dogfood assertion that the cross-dogfood step itself doesn't flag the sibling at the initial pinned SHA.

Acceptance of this RFC requires all four architect lenses to RATIFY after reviewing §5.

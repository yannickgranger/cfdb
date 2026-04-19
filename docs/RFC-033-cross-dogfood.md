---
title: RFC-033 — Cross-dogfood discipline with graph-specs-rust
status: Draft
date: 2026-04-19
authors: cfdb-architects team (drafted by team-lead)
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

### §3.1 — `.cross-fixture.toml`

Single file at the repo root of each repo:

```toml
# .cross-fixture.toml — cross-dogfood fixture pins per RFC-033.
# Bumped by the weekly cross-bump job (CI scheduled) or manually in an
# RFC-033-gated PR. The pinned SHA is expected to be clean on both tools.

[sibling]
# For cfdb: the graph-specs-rust commit to test against.
# For graph-specs-rust: the cfdb commit to test against.
repo    = "yg/graph-specs-rust"          # or "yg/cfdb" in the sibling
branch  = "develop"                       # documentation only; SHA is authoritative
sha     = "0000000000000000000000000000000000000000"
bumped_at = "2026-04-19T00:00:00Z"
bumped_by = "initial"
```

Format is deliberately minimal. CI parses with a one-line `sed` / `grep` — no TOML crate needed on the CI step.

### §3.2 — CI cross-dogfood step

Added to `.gitea/workflows/ci.yml` in both repos, after the existing self-dogfood step.

For cfdb:

```yaml
- name: Cross-dogfood — graph-specs on cfdb
  env:
    GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
  run: |
    cd repo
    SIBLING_SHA=$(grep '^sha' .cross-fixture.toml | head -1 | cut -d'"' -f2)
    git config --global url."https://oauth2:${GITHUB_TOKEN}@agency.lab:3000/".insteadOf "https://agency.lab:3000/"
    git clone https://agency.lab:3000/yg/graph-specs-rust.git /tmp/sibling
    (cd /tmp/sibling && git checkout "$SIBLING_SHA")
    # Run OUR tool against THEIR tree.
    ./target/release/cfdb extract --workspace /tmp/sibling --db .cfdb/db --keyspace sibling
    for rule in examples/queries/arch-ban-*.cypher; do
      ./target/release/cfdb violations --db .cfdb/db --keyspace sibling --rule "$rule"
    done
```

For graph-specs-rust:

```yaml
- name: Cross-dogfood — graph-specs on cfdb
  env:
    GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
  run: |
    cd repo
    SIBLING_SHA=$(grep '^sha' .cross-fixture.toml | head -1 | cut -d'"' -f2)
    git config --global url."https://oauth2:${GITHUB_TOKEN}@agency.lab:3000/".insteadOf "https://agency.lab:3000/"
    git clone https://agency.lab:3000/yg/cfdb.git /tmp/sibling
    (cd /tmp/sibling && git checkout "$SIBLING_SHA")
    # Run OUR tool against THEIR tree. The sibling tree must have spec files
    # that match ITS own code; the cross-dogfood asserts graph-specs can
    # consume arbitrary Rust + specs, not that the sibling has drift.
    ./target/release/graph-specs check \
      --specs /tmp/sibling/specs/concepts/ \
      --code /tmp/sibling/crates/
```

Both invocations must exit 0. Non-zero = cross-drift, merge blocked.

### §3.3 — Bump protocol

**Weekly automatic bump:**

A scheduled CI workflow (cron, Monday 06:00 UTC) in each repo:

1. Clones the sibling at current `develop` HEAD.
2. Runs the local tool against it.
3. If exit 0: opens a PR that updates `.cross-fixture.toml` with the new SHA.
4. If non-zero: opens a `cross-drift` issue with the failing invocation and sibling SHA.

**Manual bump:** any contributor can open a PR bumping the pin when they know the sibling has landed a change they need to consume. The PR must include:

- The new SHA.
- A one-line rationale (e.g. "picks up new `:Visibility` fact kind from cfdb #35").
- CI must pass on the bump PR just like any other.

**Schema-version lockstep:** cfdb PRs that bump `cfdb_core::SchemaVersion` MUST also include a draft PR on graph-specs-rust bumping `.cross-fixture.toml` to the cfdb PR's HEAD SHA. Neither side merges alone — they merge in order (cfdb first, then graph-specs fixture bump within minutes). The lockstep is author discipline, not a mechanical CI gate; violating it triggers a `cross-drift` issue immediately after the next weekly bump job.

### §3.4 — Zero-false-positive invariant

Enforced at CI time by the cross-dogfood step: if a new Cypher rule or equivalence-level change flags findings on the sibling tree at the pinned fixture SHA, the step exits non-zero and the PR is blocked.

Escape hatch is **intentionally narrow**: the violating PR must either (a) fix the finding in the sibling repo in a coordinated prior PR, or (b) scope the rule more narrowly to exclude the false-positive shape. There is no allowlist, no ignore-file, no exemption.

If a rule is genuinely correct and the sibling is the one with real debt, the fix is to file an issue on the sibling and land a fix there FIRST, then bump the fixture SHA to consume it.

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

Separate from the pin-bump job (§3.3), a weekly job runs cross-dogfood at HEAD of both sibling repos (NOT pinned):

1. cfdb clones graph-specs develop HEAD, runs cfdb tool, asserts zero violations.
2. graph-specs clones cfdb develop HEAD, runs graph-specs check, asserts zero violations.

Any failure opens an issue `cross-drift-YYYY-WW` in the failing repo with the failing invocation, full output, and both sibling SHAs. Neither repo's next PR merges until the drift issue is resolved.

This job catches the window between "sibling landed a change" and "our next manual pin bump" — if a sibling change would break us at HEAD, we know within a week, not at rescue time.

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

Open question: does `.cross-fixture.toml` belong at the repo root (visible, one-line bumpable) or under `.cfdb/` (hidden, near other infrastructure config)?

Open question: does the cross-dogfood CI step live in `.gitea/workflows/ci.yml` alongside the self-dogfood step, or does it deserve its own workflow file (`ci-cross.yml`) for topology clarity?

**Verdict (pending):**

### §5.2 — DDD (`ddd-specialist`)

Open question: is "sibling" the right vocabulary, or should the two tools be modelled as a single bounded context with two deployment artefacts? The current RFC treats them as separate bounded contexts with a shared kernel (the cfdb output format that graph-specs consumes).

Open question: is there a homonym risk between cfdb's `:Finding` and graph-specs' `Violation`? Both describe "something is wrong"; the difference is classifier-output vs. real-time-check-output. The RFC does not unify them — is that the right call?

**Verdict (pending):**

### §5.3 — SOLID (`solid-architect`)

Open question: the `.cross-fixture.toml` format is minimal (one-line `sed`/`grep` parseable) but could grow if more fields are needed. Should it be parsed via `toml` crate from day one, with the rationale that the one-time dependency cost is paid once? Or stays `sed`-able forever?

Open question: the bump-protocol job (§3.3) has three responsibilities: clone, test, open-PR-or-issue. SRP violation worth splitting, or is the cohesion "weekly cross-fixture maintenance" tight enough to keep as one?

**Verdict (pending):**

### §5.4 — Rust systems (`rust-systems`)

Open question: cross-dogfood CI clone + build of the sibling is potentially expensive (~2 minutes per run on cold cache). Should the pinned SHA's pre-built artefacts be cached (e.g. in Redis sccache), or is the ~2 minute cost acceptable?

Open question: `cargo install --git` of cfdb on graph-specs' CI uses `--branch develop`. Should the cross-dogfood step use a DIFFERENT SHA (from `.cross-fixture.toml`) vs. the CI-install SHA, or are they coupled?

**Verdict (pending):**

## §6 — Non-goals

1. Not publishing either tool to crates.io. The pinned-git-dep + cross-fixture model is the release mechanism for this paired toolchain.
2. Not pinning a qbot-core SHA workspace-wide in cfdb CI. qbot-core is the rescue target, not a rescue tool. Per-rescue-PR `Tests:` prescriptions pin qbot-core SHAs as needed for that PR's assertion.
3. Not introducing bidirectional schema invariants where graph-specs emits a schema that cfdb must consume. The flow is one-directional: cfdb emits facts, graph-specs consumes via `cfdb violations`.
4. Not gating cfdb's develop branch on graph-specs' CI being green (and vice versa). Each repo's CI is authoritative for its own develop; the cross-dogfood is per-PR, not per-branch.
5. Not requiring the weekly closed-loop job to auto-remediate. It opens an issue; humans fix.

## §7 — Issue decomposition

One vertical slice per issue. Each carries the `Tests:` line prescribed by the architect team after review.

**Group A — fixture file + CI wiring (cfdb):**

- Issue A1: Add `.cross-fixture.toml` to cfdb root with initial graph-specs-rust SHA. One-line schema, documented format.
- Issue A2: Wire cross-dogfood CI step in cfdb's `.gitea/workflows/ci.yml`. Exits 0 if `cfdb violations` on graph-specs-rust at pinned SHA returns zero rows.

**Group B — fixture file + CI wiring (graph-specs-rust):**

- Issue B1: Add `.cross-fixture.toml` to graph-specs-rust root with initial cfdb SHA. Mirror of A1.
- Issue B2: Wire cross-dogfood CI step in graph-specs-rust's CI. Exits 0 if `graph-specs check` on cfdb at pinned SHA returns zero violations.

**Group C — bump protocol:**

- Issue C1: Weekly cron workflow in both repos that attempts a pin bump to sibling's develop HEAD. Opens PR on success, issue on failure.
- Issue C2: Document the manual bump protocol in a new `docs/cross-fixture-bump.md` runbook.

**Group D — schema-version lockstep:**

- Issue D1: Document the `cfdb::SchemaVersion` bump protocol in cfdb's RFC §4 invariants section and reference it from graph-specs-rust's CLAUDE.md.

**Group E — closed-loop housekeeping:**

- Issue E1: Weekly cron workflow in each repo that runs cross-dogfood at BOTH SHAs `develop` HEAD (not pinned). Failure opens a `cross-drift-YYYY-WW` issue.

**Group F — `Tests:` prescription template:**

- Issue F1: Extend CLAUDE.md §2.5 in both repos to include the cross-dogfood line in the `Tests:` template.

Each issue's `Tests:` section is prescribed by the architect team during review (§5). Default: unit test for fixture parsing, integration test running the cross-dogfood step on a fresh checkout, dogfood assertion that the cross-dogfood step itself doesn't flag the sibling at the initial pinned SHA.

Acceptance of this RFC requires all four architect lenses to RATIFY after reviewing §5.

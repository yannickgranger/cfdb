# cfdb — CLAUDE.md

Repo-local rules. Extends the global `~/.claude/CLAUDE.md`, does not replace it.

## §1 — Core methodology

**New code is RFC-first. After RFC, issues. Dogfood enforcement on every PR.**

| Work type | Path |
|---|---|
| New capability (new verb, new fact type, new schema field, new `--flag`, new sub-backend) | RFC → architect review → issues → dogfood-gated PRs |
| Bug fix (wrong behavior on existing verb) | Issue → `/work-issue-lib` → dogfood-gated PR |
| Mechanical (rename, file split, dedup) | Issue → `/fix-mechanical` → dogfood-gated PR |
| Docs, CI config, chore | Issue → direct PR → dogfood-gated PR |

"RFC-first" means: no implementation issue is filed until the RFC is ratified. cfdb already has this convention de-facto (`docs/RFC-cfdb.md`, `docs/RFC-030-anti-drift-gate.md`, `docs/RFC-031-audit-cleanup.md`, `docs/RFC-032-v02-extractor.md`) — this document makes it mandatory.

## §2 — RFC pipeline

### §2.1 — Where RFCs live

`docs/RFC-<topic>.md` for major RFCs (follow existing `RFC-cfdb.md` / `RFC-030-*.md` convention). Numbered-series RFCs use `RFC-NNN-<kebab-title>.md`.

### §2.2 — RFC contents

Every RFC answers, in this order:

1. **Problem.** What concrete capability gap or anti-drift failure prompts this RFC? Cite the session, issue, or cfdb query that surfaced the need.
2. **Scope.** Exact deliverables — what ships, what does not.
3. **Design.** Types (Node/Edge additions), wire format additions, Cypher subset additions, CLI verb signature, schema version bump if any.
4. **Invariants.** Determinism (sha256 byte-stable re-extract), recall (extractor ≡ rustdoc-json ground truth), no-ratchet rule (§6.8), keyspace backward-compat.
5. **Architect lenses.** Dedicated subsections for each architect perspective (see §2.3). Architects' verdicts are captured inline.
6. **Non-goals.** Explicit.
7. **Issue decomposition.** Vertical slices, one issue each. Each entry carries an explicit `Tests:` line naming the test surface per §2.5 — architects prescribe, implementers execute.

Ratified RFCs live alongside drafts. The `council/RATIFIED.md` and `council/verdicts/` pattern already shows how cfdb records architect verdicts.

### §2.3 — Architect review via agent teams

Reference: https://code.claude.com/docs/en/agent-teams.

Every RFC is reviewed by a team of architect sub-agents, one teammate per lens:

| Lens | Subagent type | Question answered |
|---|---|---|
| Clean architecture | `clean-arch` | StoreBackend trait purity, crate dependency direction, composition root |
| Domain-driven design | `ddd-specialist` | Node/Edge vocabulary, bounded contexts, homonym detection on `:Item` / `:CallSite` |
| SOLID + component principles | `solid-architect` | Crate granularity, SRP on evaluator vs extractor, stable abstractions for `cfdb-core` |
| Rust systems | `rust-systems` | `syn` parsing strategy, petgraph internals, feature flags, trait object safety |

Invocation is via `Agent(subagent_type=...)` or agent teams. Each lens returns a verdict (RATIFY / REJECT / REQUEST CHANGES) with evidence. The RFC is not ratified until all four verdicts are RATIFY, or a single author-documented override is recorded in `council/RATIFIED.md`.

**Architects also prescribe tests** (§2.5). The verdict is not complete until each issue in the decomposition carries a named test surface — unit, integration, recall-corpus extension, dogfood assertion, or a documented `Tests: none` rationale. Implementers do not choose the test shape; they deliver against the prescription.

The existing `council/BRIEF.md` / `council/SYNTHESIS-R1.md` / `council/RATIFIED.md` artifacts are the model for this — make the pattern mandatory for new capability, not optional.

### §2.4 — Ratification → issues

Once ratified, the RFC's "Issue decomposition" section becomes the concrete backlog. Each vertical slice is filed as a `forge_create_issue` with body linking back to the RFC (`Refs: docs/RFC-<name>.md`) and carrying the prescribed `Tests:` section from the RFC verbatim. Issues are worked via `/work-issue-lib`. A PR against an issue without the prescribed test is not merged.

### §2.5 — Tests and real infra

**Tests are always mandatory when possible.** "When possible" = there is an executable path the change touches that can be exercised deterministically. "Mandatory" = the PR implementing the issue lands the prescribed test; a PR without it is not merged. Architects prescribe in the RFC + issue body (§2.3 + §2.4); implementers pass.

**Real infra is always preferred over mocks.** The hierarchy:

1. **Dogfood / self-integration.** Exercise the change against cfdb's own source tree via `cfdb extract --workspace .` and assert an invariant (e.g. "the new `:Visibility` attribute is emitted for ≥ N% of pub items in our own crates"). This is the strongest signal because it uses real data flowing through the real pipeline.
2. **Integration against real inputs.** Construct a small real-shaped input (a synthetic cargo workspace fixture, a concrete `.cypher` rule file) and run the full pipeline end-to-end. Assert on the observable output.
3. **Unit tests on pure functions.** Fine when the function is genuinely pure (values in → values out, zero I/O). Do not stub out I/O that could be exercised via option 2.
4. **Mocks / doubles.** Last resort. Must carry an inline comment naming why real infra was unavailable (e.g. nightly-only ground truth absent in CI).

**Prescribed test categories by work type:**

| Work type | Required test |
|---|---|
| New capability (new verb, fact type, schema field) | Dogfood-against-cfdb assertion **AND** unit tests for extracted pure functions **AND** `cfdb-recall` corpus extension when the change adds a new fact kind |
| Bug fix | Regression test that reproduces the bug first (red → green in the same PR) |
| Mechanical refactor | No new tests; the existing suite must pass byte-identically (the invariant the refactor preserves) |
| Docs / CI / chore | No test required; the change is its own verification surface |

**Escape hatch.** An issue that is genuinely untestable carries `Tests: none — rationale: <why>` in its body. "I didn't bother" is not a valid rationale. Examples that DO qualify: a typo fix in a comment; a README paragraph rewrite. Examples that do NOT: "it's just a small refactor" (mechanical refactor still requires the existing suite to pass); "the CI test will catch it" (the prescribed test IS the signal, not a hope about CI).

## §3 — Dogfood enforcement

Every PR passes these gates. CI enforces them.

| Gate | Tool | Question answered | Failure mode |
|---|---|---|---|
| Self-hosted ban rules | `cfdb violations` against `.cfdb/queries/*.cypher` (cfdb run on cfdb) | "Does cfdb's own code use forbidden patterns?" | Any new row under a ban rule |
| Extractor recall | `cfdb-recall` (extractor vs `rustdoc --output-format=json`) | "Does the syn-based extractor see everything rustdoc sees?" | Missing items, missing edges, missing call sites |
| Determinism | `ci/determinism-check.sh` | "Is `cfdb extract` byte-stable on an unchanged tree?" | sha256 mismatch across two extracts |
| Cross-dogfood | `ci/cross-dogfood.sh` against companion at pinned SHA | "Does cfdb still produce zero findings on graph-specs-rust?" | Any rule row on companion → exit 30; see [docs/cross-fixture-bump.md](docs/cross-fixture-bump.md) |
| No metric ratchets | Global rule (`~/.claude/CLAUDE.md` §6.8) | "Does this PR introduce a baseline / ceiling / allowlist file?" | PR rejected on sight |

**Adding a new ban rule is an RFC-gated change.** The rule goes into the same PR as the code motivating it, with proof that develop is zero-violation before the rule lands.

**Adding a new fact type, Cypher subset construct, or schema field is RFC-gated.** The schema vocabulary in `cfdb-core` is the source of truth — changes here invalidate downstream keyspaces.

Downstream consumption: `agency:yg/graph-specs-rust` vendors cfdb as a pinned git dep (see graph-specs-rust `.cfdb/cfdb.rev`). Schema breakage there is a cross-repo coordination cost — bump `schema_version` visibly and give downstream one release of overlap when possible.

## §4 — Skill selection

| Scenario | Skill |
|---|---|
| New vertical slice derived from a ratified RFC | `/work-issue-lib` |
| Bug fix on existing behavior | `/work-issue-lib` |
| Rename / move / dedup / file split | `/fix-mechanical` |
| N parallel mechanical refactors | `/sweep-epic` |
| Pre-push | `/ship` — the only authorized push + PR path |

## §5 — Schema discipline

`cfdb-core::SchemaVersion` is the wire contract for every keyspace on disk. Breaking changes MUST bump it. Non-breaking additions (new node label, new edge label, new optional attribute) MAY keep the version but SHOULD be called out in RFC §4 (Invariants) and `SchemaDescribe` output.

The `cfdb-recall` crate holds cfdb to the rustdoc ground truth. Any RFC that adds a new fact type MUST extend the recall corpus before merge — otherwise the new fact is unverified against a source of truth.

## §6 — Quick reference

```bash
# Build
cargo build -p cfdb-cli --release

# Self-dogfood — extract cfdb and run its own ban rules against itself
./target/release/cfdb extract --workspace . --db .cfdb/db --keyspace cfdb
for r in .cfdb/queries/*.cypher; do ./target/release/cfdb violations --db .cfdb/db --keyspace cfdb --rule "$r"; done

# Determinism
./ci/determinism-check.sh

# Ship
/ship <issue> agency:yg/cfdb --workspace <path>
```

## §7 — Companion policy

The same RFC-first + architect-review methodology applies to `yg/graph-specs-rust`. See that repo's `CLAUDE.md`.

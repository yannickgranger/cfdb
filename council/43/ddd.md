# DDD Verdict — Issue #43 Decomposition

**Verdict:** REQUEST CHANGES
**Reviewer:** ddd-specialist agent
**Date:** 2026-04-20
**Worktree:** .claude/worktrees/43-enrichment @ 1659e2a

---

## Q1 — Vocab reconciliation (trait names vs RFC 5 passes)

### Evidence base

Current `EnrichBackend` trait in `crates/cfdb-core/src/enrich.rs` (lines 80–111) exposes four methods:

| Trait method | CLI verb (cfdb-cli/src/main.rs:119–147) | Phase A stub |
|---|---|---|
| `enrich_docs` | `enrich-docs` | yes |
| `enrich_metrics` | `enrich-metrics` | yes |
| `enrich_history` | `enrich-history` | yes |
| `enrich_concepts` | `enrich-concepts` | yes |

RFC addendum §A2.2 (lines 199–204) names five passes for Stage 1 enrichment:

| RFC pass name | Output |
|---|---|
| `enrich_git_history` | `:Item.git_age_days`, `:Item.git_last_author`, `:Item.git_commit_count` |
| `enrich_rfc_docs` | `(:Item)-[:REFERENCED_BY]->(:RfcDoc)` |
| `enrich_deprecation` | `:Item.is_deprecated`, `:Item.deprecation_since` |
| `enrich_bounded_context` | `:Item.bounded_context`, `(:Crate)-[:BELONGS_TO]->(:Context)` |
| `enrich_reachability` | `:Item.reachable_from_entry`, `:Item.reachable_entry_count` |

### Name-pair analysis

**`enrich_history` ↔ `enrich_git_history`**: Same concept. The current stub doc string at `enrich.rs:96` says "Enrich a keyspace with git-history facts (last-touched, churn, author)" — verbatim the RFC §A2.2 row. The RFC's longer name (`enrich_git_history`) is more precise: it qualifies *which* history source (git), which matters because future versions might include changelog or PR history. The RFC name should be canonical.

**`enrich_docs` ↔ `enrich_rfc_docs`**: Partially the same, but with a critical semantic shift. The current `enrich_docs` stub doc (`enrich.rs:82–85`) says "rustdoc, README, RFC text" — a broader concept. The RFC §A2.2 scope is narrower: "keyword match against concept names" in `.concept-graph/` + `docs/rfc/*.md`. The RFC pass is RFC-reference enrichment only; rustdoc rendering is not in the five-pass table. This is a genuine semantic split. See Finding 1 below.

**`enrich_concepts` ↔ `enrich_bounded_context`**: Different concept. The current stub says "bounded-context / concept facts" (enrich.rs:101–102), conflating two distinct responsibilities: (a) assigning `bounded_context` per item, and (b) materializing `:Concept` nodes from TOML declarations. The RFC decomposes these correctly — `enrich_bounded_context` handles the context assignment; `:Concept` node materialization is a separate concern (see Q4). The RFC name is canonical.

**`enrich_metrics`**: Has NO counterpart in the RFC §A2.2 five-pass table. The `describe.rs:62–75` schema descriptor shows it populates `cyclomatic`, `dup_cluster_id`, `test_coverage`, `unwrap_count` on `:Item` — quality signals, not enrichment-pipeline signals. The addendum §A2.2 explicitly says these are "quality tools" orthogonal to the debt-cause classifier pipeline. `enrich_metrics` is NOT dropped — it is deferred to a separate scope (v0.2 quality metrics, not the five-pass classifier). Issue #43 must NOT implement `enrich_metrics`; it is a separate concern.

**`enrich_reachability` and `enrich_deprecation`**: Both are new RFC passes with NO current stub. They must be added as new methods on `EnrichBackend`.

### Decision table

| Action | Trait method | Rationale |
|---|---|---|
| RENAME | `enrich_history` → `enrich_git_history` | RFC name is more precise; git source qualifier is semantically load-bearing |
| RENAME + NARROW | `enrich_docs` → `enrich_rfc_docs` | RFC scope is RFC-reference enrichment only; broader rustdoc enrichment is out of scope for #43 |
| RENAME | `enrich_concepts` → `enrich_bounded_context` | RFC name is the correct scope; concept materialization is separate |
| ADD | (new) `enrich_deprecation` | No current stub; RFC §A2.2 row 3 |
| ADD | (new) `enrich_reachability` | No current stub; RFC §A2.2 row 5 |
| DEFER (not in #43) | `enrich_metrics` | Not in RFC §A2.2 classifier pipeline; separate quality concern |

**RFC amendment required.** The rename of `enrich_docs` to `enrich_rfc_docs` narrows the published Language surface of `EnrichBackend`. The broader rustdoc enrichment implied by the Phase A stub comment is not in the RFC addendum §A2.2 table at all — it is an unscoped aspiration in the Phase A docstring. The RFC must explicitly record that (a) `enrich_docs` → `enrich_rfc_docs` with scope limited to RFC-file keyword matching, and (b) full rustdoc rendering enrichment is deferred to a future pass (not v0.2). This must be a named non-goal in the RFC.

**Trait renaming is a breaking change** on any downstream implementing `EnrichBackend`. `cfdb-cli/src/enrich.rs:14–19` dispatches via `EnrichVerb::Docs/History/Concepts` — all three match arms must change. The CLI verb wire-form test at `crates/cfdb-cli/tests/wire_form_17_verbs.rs` includes `("enrich_concepts", "enrich-concepts")` — that test must be updated. Any user with a persisted `EnrichReport.verb` string equal to `"enrich_concepts"` will see a mismatch. SchemaVersion does not govern `EnrichReport.verb` strings (they are not graph schema), but callers checking `report.verb` by name will break. This is explicitly the level of breakage the RFC-first rule exists to manage — the RFC must acknowledge it.

---

## Q2 — Node/edge additions per pass

### Full table

| Pass | New attributes / labels / edges | Already reserved in `schema/labels.rs`? | `SchemaVersion` bump required? |
|---|---|---|---|
| `enrich_git_history` | `:Item.git_age_days` (int), `:Item.git_last_author` (string), `:Item.git_commit_count` (int) | NOT reserved. No `git_age_days` constant in `labels.rs` (lines 17–115). Not in `describe.rs` `node_descriptors()` (lines 24–156). | YES — additive attribute addition. Patch bump (v0.2.1) per §5 CLAUDE.md "non-breaking additions MAY keep the version but SHOULD be called out". The `git_age_days` attribute is a new observable fact kind; V0_2_0 readers querying it on a V0_2_0 graph without the pass having run will get null. Patch bump signals the new attributes exist. |
| `enrich_rfc_docs` | `(:Item)-[:REFERENCED_BY]->(:RfcDoc)` — new edge label `REFERENCED_BY`, new node label `:RfcDoc {path, title}` | NEITHER reserved. `REFERENCED_BY` is absent from `EdgeLabel` constants (lines 74–108). `:RfcDoc` is absent from `Label` constants (lines 18–44) and from `node_descriptors()`. | YES — both a new node label and a new edge label. Patch bump (could be combined with git_history bump). |
| `enrich_deprecation` | `:Item.is_deprecated` (bool), `:Item.deprecation_since` (string?) | NOT reserved. Not in `describe.rs` `:Item` attribute list (lines 58–78). | YES — additive attributes. Patch bump. |
| `enrich_bounded_context` | `:Item.bounded_context` (string), `(:Crate)-[:BELONGS_TO]->(:Context)` | PARTIALLY reserved. `bounded_context` appears in `describe.rs:60` as an existing `Extractor`-provenanced attribute on `:Item`. `BELONGS_TO` is reserved as `EdgeLabel::BELONGS_TO` at `labels.rs:85`. `:Context` is reserved as `Label::CONTEXT` at `labels.rs:45`. The `BELONGS_TO` edge is described in `describe.rs:237–244` targeting `:Context`. **`enrich_bounded_context` does NOT add new schema** — it already runs at extraction time (`cfdb-extractor/src/lib.rs:105–142` emits `bounded_context` on every `:Item` and `BELONGS_TO` on every crate). | NO — schema already exists and is populated. `enrich_bounded_context` as a Phase D pass would be a re-enrichment or override of data already written at extraction. See Q3 for implications. |
| `enrich_reachability` | `:Item.reachable_from_entry` (bool), `:Item.reachable_entry_count` (int) | NOT reserved. Not in `describe.rs`. Note: `cfdb-query/src/inventory.rs:3` has `pub reachable_from_entry_point: bool` — but that is a Rust struct field in the query layer, not a graph attribute. | YES — additive attributes. Patch bump. |

### Summary on SchemaVersion

Four of the five passes require a `SchemaVersion` patch bump. They can be batched into a single v0.2.1 bump IF they land in the same PR. If they land in separate PRs, each must bump independently (or the multi-pass PR strategy must be declared in the RFC). The RFC addendum §A2.2 does not name a schema version bump — this is a missing specification that must be added before implementation begins.

Each bump requires a lockstep PR on `agency:yg/graph-specs-rust` per CLAUDE.md §3 and `docs/cross-fixture-bump.md`.

---

## Q3 — Bounded context homonym test

### RFC §A2.1 class 2 definition

RFC addendum §A2.1 (line 184) defines Context Homonym as: "Same name (or high-Jaccard structural similarity) appearing in items whose owning crates belong to **different bounded contexts**, where the semantics diverge." The signal `a.bounded_context <> b.bounded_context` is required for the classifier to fire class 2.

### What happens at 80% accuracy

If `enrich_bounded_context` produces 80% accurate assignments (20% noise), two failure modes emerge:

**False positives (homonym firing on non-homonyms):** Two items in the same context but mis-assigned to different contexts will be classified as a Context Homonym and routed to `/operate-module` with `council_required=true`. This triggers an expensive architectural deliberation for a mechanical dedup that `/sweep-epic` could have handled. At 20% error rate across the graph, this is not a rare edge case — it is systematic inflation of the council queue.

**False negatives (homonyms missed):** Two items in genuinely different contexts but both mis-assigned to the same context will be classified as class 1 (DuplicatedFeature) and routed to `/sweep-epic --consolidate`. This is worse: the sweep will DELETE one implementation, believing they are functionally identical, when in fact they serve different domain invariants. The RFC §A2.1 fix strategy for class 1 is "pick one head, delete the loser" — applied to a true homonym, this is a domain model corruption, not a cleanup.

### RFC §A3.2 threshold behavior

RFC §A3.2 (line 298) states ">3 context-homonym findings triggers surgery regardless of other counts." At 80% accuracy, the false-positive homonym count will exceed this threshold in any reasonably-sized workspace (qbot-core has 23+ crates). This means `/operate-module` will be triggered on bounded contexts that are not infected, consuming architect time on phantom findings. The threshold was designed assuming accurate context assignments.

### The RFC §A2.2 classifier has no confidence-gating

The classifier Cypher (RFC §A2.2 lines 208–232) uses `cross_context` as a hard boolean: `a.bounded_context <> b.bounded_context`. There is a `confidence_score(a, b)` UDF referenced in the output but it is not defined in the RFC and is not used to gate the class assignment — it is metadata only. The RFC provides no downgrade path (e.g., "if confidence < threshold, emit `unknown_class` instead of `context_homonym`").

### Conclusion

The RFC v0.2-9 gate item (RFC addendum line 163) addresses this: "Gate passes if ≥95% of items receive the human-expected context label." This gate is correctly positioned as a gate on `enrich_bounded_context` — it must pass at ≥95% before the classifier is run in anger. The DDD lens endorses this gate as load-bearing. The 95% threshold is the minimum defensible floor; below it the classifier produces more noise than signal for the Context Homonym class. Slices implementing the classifier (two-stage pipeline) must declare a hard dependency on the v0.2-9 gate passing first — this dependency must appear in each slice's blockers list.

---

## Q4 — `:Concept` node materialization

### Current state

The current `enrich_concepts` stub at `crates/cfdb-core/src/enrich.rs:101–108` is documented as "bounded-context / concept facts". This conflates two distinct operations in a single stub:

1. **Context assignment** — resolved by `cfdb-concepts` crate and already running at extraction time (see `cfdb-extractor/src/lib.rs:105–142`). This is NOT what Phase D needs to implement for `enrich_bounded_context`.

2. **Concept node materialization** — emitting `:Concept` nodes from `.cfdb/concepts/*.toml` declarations and connecting `:Item` nodes to them via `LABELED_AS` and `CANONICAL_FOR` edges (reserved in `labels.rs:97–98`). This is a genuine Phase D enrichment pass.

### What the extractor currently emits

`cfdb-extractor/src/lib.rs:166–189` already emits `:Context` nodes (label `Label::CONTEXT`) and `BELONGS_TO` edges for each bounded context. It does NOT emit `:Concept` nodes — those require reading the concept TOML files' `canonical_crate` and `owning_rfc` metadata and linking them to `:Item` nodes. The `ConceptOverrides` struct in `cfdb-concepts/src/lib.rs:77–99` provides the data but the extractor only uses it for context assignment, not for `:Concept` node emission.

### Issue #101's dependency

Issue #101 is described as "Trigger T1 (concept-declared-in-toml-but-missing-in-code) — consumes `:Concept` nodes." This implies `:Concept` nodes exist in the graph before #101 runs. These nodes must come from somewhere.

The `describe.rs:138–145` schema descriptor declares `:Concept` attributes (`assigned_by`, `name`) both with `Provenance::EnrichConcepts`. This means `:Concept` node materialization is the responsibility of the enrichment pass, NOT the extractor.

### Resolution

**`:Concept` node materialization is NOT part of `enrich_bounded_context`.** It is a sixth enrichment operation that should be named `enrich_concepts` — but with a precise scope different from the current Phase A stub's vague scope. The precise scope is: for each `.cfdb/concepts/<name>.toml` file, emit one `:Concept {name, assigned_by: "manual"}` node and emit `LABELED_AS` edges connecting `:Item` nodes (by qname or pattern) declared as canonical in the TOML to that `:Concept` node.

**The current `enrich_concepts` stub name is correct** — but its documentation is misleading because it mixes context assignment (already done at extraction) with concept node materialization (not yet done). The Phase D implementation of `enrich_concepts` should focus exclusively on concept node materialization from TOML declarations.

**Issue #101 is blocked on a correctly implemented `enrich_concepts` pass**, not on `enrich_bounded_context`. The RFC §A2.2 five-pass table omits this sixth pass — this is a gap in the RFC that must be corrected before ratification.

**Recommendation:** Add a sixth enrichment pass row to RFC §A2.2:

| Pass | Input | Output | Layer |
|---|---|---|---|
| `enrich_concepts` | `.cfdb/concepts/*.toml` + `:Item` node lookup by qname/pattern | `(:Concept {name, assigned_by})` nodes + `(:Item)-[:LABELED_AS]->(:Concept)` + `(:Item)-[:CANONICAL_FOR]->(:Concept)` | enricher (reads TOML declarations already loaded by `cfdb-concepts`) |

This sixth pass is a prerequisite for issues #101 and #102 and must be sliced as a child of #43.

---

## Q5 — Proposed slice decomposition

The five RFC passes and the missing sixth pass decompose into seven child issues. Ordering is dictated by data dependencies: `bounded_context` and `enrich_concepts` must be ready before the two-stage classifier can run.

---

### Slice A — Rename/reshape `EnrichBackend` trait + add two missing stubs

**Scope:** Mechanical rename (`enrich_history` → `enrich_git_history`, `enrich_docs` → `enrich_rfc_docs`, `enrich_concepts` → `enrich_bounded_context`) + add two new stub methods (`enrich_deprecation`, `enrich_reachability`) + add sixth stub (`enrich_concepts` with narrowed scope) + update all callers in `cfdb-cli`. This is a `/fix-mechanical` slice — no behavior changes, no graph mutations.

**Nodes/edges/attributes written:** None (stubs only).

**Accuracy gate:** N/A — mechanical.

**Blockers:** None. Must land first; all other slices depend on the renamed trait.

**Parallelism:** Blocks all other slices; cannot parallelize with any of them.

**RFC amendment required:** Yes — record the `enrich_docs` → `enrich_rfc_docs` scope narrowing and the sixth `enrich_concepts` pass addition as named scope changes. Without RFC amendment, the rename violates the RFC-first rule.

**Tests:**
- Unit: none (mechanical rename — existing suite must pass byte-identically)
- Self dogfood (cfdb on cfdb): `cfdb enrich-git-history --db .cfdb/db --keyspace cfdb` returns `{ran: false}` stub report with updated verb name
- Cross dogfood (cfdb on graph-specs-rust at pinned SHA): none — no graph output
- Target dogfood (on qbot-core at pinned SHA): none — no graph output

---

### Slice B — `enrich_git_history` implementation

**Scope:** Implement `enrich_git_history` pass. Walk `:Item` nodes in the keyspace; for each, resolve its `file` attribute; call `git log --follow -1 --format="%ad %ae" -- <file>` (or `git2` Rust crate for subprocess-free access per RFC §A2.2 "uses git2 crate"); write `git_age_days`, `git_last_author`, `git_commit_count` attributes to each `:Item` node. Bump SchemaVersion to v0.2.1 (or coordinate with other attribute-adding slices into a single bump).

**Nodes/edges/attributes written:**
- `:Item.git_age_days` (int) — days since last git touch of the containing file
- `:Item.git_last_author` (string) — email of last commit author
- `:Item.git_commit_count` (int) — commit count for the containing file

**Accuracy gate:** no RFC gate item for git history specifically; correctness is validated by the self-dogfood assertion.

**Blockers:** Slice A (trait rename). SchemaVersion bump coordination with Slices C, D.

**Parallelism:** Can run in parallel with Slices C and D after Slice A lands.

**Tests:**
- Unit: given a synthetic git repo fixture with 2 commits on a file, assert `git_age_days >= 0`, `git_last_author` matches committer email, `git_commit_count == 2`
- Self dogfood (cfdb on cfdb): `cfdb enrich-git-history --db .cfdb/db --keyspace cfdb` → assert `attrs_written >= 1`, `ran == true`; spot-check `cfdb-core/src/enrich.rs` item has `git_commit_count > 0`
- Cross dogfood (cfdb on graph-specs-rust at pinned SHA): run `enrich-git-history` on companion; assert zero rule violations on any new ban rules added by this slice (none expected); include schema version bump in cross-fixture bump PR
- Target dogfood (on qbot-core at pinned SHA): report count of items with `git_age_days > 365` in PR body as staleness telemetry for reviewer

---

### Slice C — `enrich_deprecation` implementation

**Scope:** Implement `enrich_deprecation` pass. Walk `:Item` nodes; re-read each item's `file:line` in the AST (or store the raw `#[deprecated]` attribute text at extraction time via a new `Extractor`-provenanced attribute); extract `is_deprecated = true` and `deprecation_since` (the `since` field of the attribute if present). The RFC §A2.2 notes this "reuses existing AST walk" — since the extractor already touches these files, a clean design emits `is_deprecated` at extraction time with `Provenance::Extractor`, not as a Phase D enrichment. The schema descriptor in `describe.rs` does not currently list these attributes — they must be added.

**Nodes/edges/attributes written:**
- `:Item.is_deprecated` (bool)
- `:Item.deprecation_since` (string?) — version string from `#[deprecated(since = "X")]`, nullable

**Accuracy gate:** no RFC gate item; correctness is the unit test coverage of all `#[deprecated]` attribute forms.

**Blockers:** Slice A (trait rename). SchemaVersion bump coordination.

**Parallelism:** Can run in parallel with Slices B and D after Slice A lands.

**DDD note:** the provenance of `is_deprecated` deserves careful consideration. The RFC §A2.2 marks this as "extractor (no new I/O; reuses existing AST walk)" — which implies `Provenance::Extractor`, not `Provenance::EnrichDeprecation`. If it is extraction-time, it should not be an enrichment pass at all; it should extend the existing `item_visitor.rs`. This distinction has classifier implications: the two-stage pipeline's Stage 2 Cypher assumes `has_deprecation` is populated before querying. If it is extraction-time, no enrichment pass is needed; if it is enrichment-time, the classifier must declare `enrich_deprecation` as a prerequisite. The RFC must resolve this before implementation. The DDD lens recommends: emit at extraction time (it is a syntactic attribute, not an I/O-bound signal); remove `enrich_deprecation` from the Phase D enrichment set; declare it as an extractor extension instead.

**Tests:**
- Unit: fixture file with `#[deprecated]`, `#[deprecated(since = "1.2.0")]`, and plain items — assert `is_deprecated` populated correctly for each case
- Self dogfood (cfdb on cfdb): assert `cfdb query` returns 0 items with `is_deprecated = true` in cfdb's own codebase (cfdb has no deprecated items — this doubles as a negative-case regression)
- Cross dogfood (cfdb on graph-specs-rust at pinned SHA): include schema version in cross-fixture bump PR
- Target dogfood (on qbot-core at pinned SHA): report count of deprecated items in PR body

---

### Slice D — `enrich_rfc_docs` implementation

**Scope:** Implement `enrich_rfc_docs` pass. Load all `.concept-graph/*.md` and `docs/rfc/*.md` files; extract concept name tokens from their content; for each `:Item` whose `name` or `qname` appears in a loaded RFC file, emit one `REFERENCED_BY` edge to a `:RfcDoc {path, title}` node. This requires two new schema additions: node label `:RfcDoc` and edge label `REFERENCED_BY`. Both are absent from `labels.rs`.

**Nodes/edges/attributes written:**
- New node: `:RfcDoc {path: string, title: string?}` — one node per unique RFC file
- New edge: `(:Item)-[:REFERENCED_BY]->(:RfcDoc)` — one edge per (item, rfc-file) pair where the item name appears in the file

**Schema additions required:**
- `Label::RFC_DOC = "RfcDoc"` constant in `labels.rs` (after `Label::CONTEXT` at line 45)
- `EdgeLabel::REFERENCED_BY = "REFERENCED_BY"` constant in `labels.rs` (after `EdgeLabel::EQUIVALENT_TO` at line 99)
- Node descriptor for `:RfcDoc` in `describe.rs`
- Edge descriptor for `REFERENCED_BY` in `describe.rs`

**Accuracy gate:** no explicit RFC gate; keyword matching precision/recall is inherently heuristic. The unit test must validate that a known item name appearing in a known RFC file produces the edge.

**Blockers:** Slice A (trait rename). SchemaVersion bump.

**Parallelism:** Can run in parallel with Slices B, C, and F after Slice A lands.

**Tests:**
- Unit: synthetic workspace + synthetic RFC file containing the item name "FooBarService" → assert one `REFERENCED_BY` edge emitted to the RfcDoc node
- Self dogfood (cfdb on cfdb): `cfdb enrich-rfc-docs` on cfdb's own workspace → assert `EnrichReport.edges_written > 0` (cfdb items appear in cfdb's own RFC files)
- Cross dogfood (cfdb on graph-specs-rust at pinned SHA): include new `:RfcDoc` label and `REFERENCED_BY` edge in cross-fixture bump PR
- Target dogfood (on qbot-core at pinned SHA): report count of items with RFC references in PR body

---

### Slice E — `enrich_bounded_context` (Phase D override pass)

**Scope:** This slice is narrower than it appears. The schema is already populated at extraction time (`cfdb-extractor/src/lib.rs:105–142` emits `bounded_context` on every `:Item` and `BELONGS_TO` on every crate). The Phase D `enrich_bounded_context` pass serves as a re-enrichment step for workspaces where the TOML overrides were added or modified after the initial extraction, without requiring a full re-extract. It re-reads `.cfdb/concepts/*.toml` and updates `bounded_context` on any `:Item` whose crate mapping changed.

**If the design is "re-extract always refreshes context"** (extraction includes TOML loading), then `enrich_bounded_context` as a Phase D pass is redundant — the data is already correct. In that case this slice is unnecessary and should be dropped from the RFC, saving one implementation slot.

**DDD recommendation:** treat `enrich_bounded_context` as a Phase D no-op for workspaces where extraction is recent, and as a targeted "refresh context assignments" pass for stale graphs. The v0.2-9 accuracy gate applies to this pass regardless of whether it is implemented as an enrichment pass or as an extractor guarantee.

**Nodes/edges/attributes written:** None new (updates existing `bounded_context` attribute and `BELONGS_TO` edges); no SchemaVersion bump unless the update logic needs to be signaled.

**Accuracy gate:** RFC v0.2-9 — ≥95% of items in the three ground-truth crates (`domain-strategy`, `ports-trading`, `qbot-mcp`) receive the human-expected context label.

**Blockers:** Slice A (trait rename). The v0.2-9 gate must pass before the two-stage classifier (Slice G) can run.

**Parallelism:** Can run in parallel with Slices B, C, D, F.

**Tests:**
- Unit: workspace fixture where `.cfdb/concepts/trading.toml` maps `domain-trading` → `trading`; assert all items from that crate have `bounded_context = "trading"`
- Self dogfood (cfdb on cfdb): run pass; assert cfdb's own items have `bounded_context = "cfdb-core"` / `"cfdb-extractor"` etc. (heuristic, no TOML override for cfdb's own workspace)
- Cross dogfood (cfdb on graph-specs-rust at pinned SHA): zero rule violations
- Target dogfood (on qbot-core at pinned SHA): manual spot-check of 3 ground-truth crates — produce the v0.2-9 one-page accuracy report in PR body; gate blocks merge if accuracy < 95%

---

### Slice F — `enrich_concepts` implementation (`:Concept` node materialization)

**Scope:** Implement the renamed `enrich_concepts` pass with its correct scope: for each `.cfdb/concepts/<name>.toml` file, emit one `:Concept {name, assigned_by: "manual"}` node; for each crate in the TOML's `crates` list, emit `LABELED_AS` edges connecting `:Item` nodes matching the crate's `canonical_crate` declaration to the `:Concept` node. This is the prerequisite for issues #101 and #102.

**Nodes/edges/attributes written:**
- `:Concept {name, assigned_by}` nodes (one per TOML file)
- `(:Item)-[:LABELED_AS]->(:Concept)` edges (items in the `canonical_crate` of the context)
- `(:Item)-[:CANONICAL_FOR]->(:Concept)` edges (items declared as canonical in the TOML)

**Schema additions:** `:Concept` and the concept overlay edges (`LABELED_AS`, `CANONICAL_FOR`, `EQUIVALENT_TO`) are already reserved in `labels.rs:44,97–99` and described in `describe.rs:138–145,293–314`. No new schema additions.

**Blockers:** Slice A (trait rename). Slice E (accurate `bounded_context` assignment, so concept-to-crate mapping is correct). Issues #101 and #102 block on this slice.

**Parallelism:** Depends on Slice E's v0.2-9 gate passing. Can otherwise run in parallel with B, C, D.

**Tests:**
- Unit: synthetic `.cfdb/concepts/trading.toml` declaring `canonical_crate = "domain-trading"` → assert one `:Concept {name: "trading"}` node emitted; assert `LABELED_AS` edges for items in `domain-trading`
- Self dogfood (cfdb on cfdb): cfdb's own workspace has no `.cfdb/concepts/` files → assert `enrich_concepts` runs with `attrs_written = 0` and `ran = true` (not a stub)
- Cross dogfood (cfdb on graph-specs-rust at pinned SHA): if companion has `.cfdb/concepts/` files, assert concept nodes emitted; otherwise assert graceful no-op
- Target dogfood (on qbot-core at pinned SHA): report count of `:Concept` nodes emitted and count of `LABELED_AS` edges in PR body; this is the prerequisite metric for #101

---

### Slice G — `enrich_reachability` implementation

**Scope:** Implement `enrich_reachability` pass. Perform BFS from every `:EntryPoint` node over `CALLS*` edges; for each reachable `:Item`, write `reachable_from_entry = true` and `reachable_entry_count = N` (count of distinct entry points that reach this item). Items not reached by any BFS write `reachable_from_entry = false`.

**Dependency:** This pass requires `:EntryPoint` nodes and `CALLS` edges to exist. `:EntryPoint` nodes are emitted by `cfdb-hir-extractor` (v0.2.0 per `describe.rs:127–137`). `CALLS` edges require HIR resolution. This means `enrich_reachability` cannot produce meaningful output without `cfdb-hir-extractor` having been run. The RFC §A2.2 marks this pass's layer as "enricher (runs after Pattern A/B extraction completes)" — this is the correct dependency statement.

**Nodes/edges/attributes written:**
- `:Item.reachable_from_entry` (bool)
- `:Item.reachable_entry_count` (int)

**Accuracy gate:** RFC §A2.1 class 6 Unwired detection depends on `reachable_from_entry = false` being accurate. Accuracy is bounded by `CALLS` edge recall (RFC v0.2-4: ≥80% against ground truth). A `reachable_from_entry = false` result when `CALLS` recall is 80% means up to 20% of items marked "unwired" may be false positives. The slice must document this in its test prescription.

**Blockers:** `cfdb-hir-extractor` producing `:EntryPoint` nodes and `CALLS` edges (issue #86 — already shipped per `describe.rs:129`). Slice A (trait rename). SchemaVersion bump.

**Parallelism:** Can start implementation after Slice A, but cannot produce meaningful test results until the HIR extractor output is available in the test fixture.

**Tests:**
- Unit: synthetic graph with 2 `:EntryPoint` nodes, 3 `:Item` nodes, `CALLS` edges making 2 of the 3 reachable — assert `reachable_from_entry = true` on 2 items, `reachable_entry_count = 1 or 2`, `reachable_from_entry = false` on the third
- Self dogfood (cfdb on cfdb): cfdb's own graph may have limited `:EntryPoint` / `CALLS` coverage (HIR extractor needed) — run pass and assert `ran = true`; report count of unreachable items in PR body
- Cross dogfood (cfdb on graph-specs-rust at pinned SHA): include new attributes in schema version bump; zero rule violations
- Target dogfood (on qbot-core at pinned SHA): report count of items with `reachable_from_entry = false` as unwired telemetry; note caveat that count is bounded by HIR extractor recall

---

### Dependency graph and parallelism

```
Slice A (trait rename — no behavior)
  |
  +-- Slice B (enrich_git_history)     \
  +-- Slice C (enrich_deprecation)      | parallel after A
  +-- Slice D (enrich_rfc_docs)        /
  |
  +-- Slice E (enrich_bounded_context) -- v0.2-9 gate must pass
  |                                         |
  +-- Slice F (enrich_concepts)  <----------+ depends on E
  |
  +-- Slice G (enrich_reachability) -- depends on HIR extractor (separate)
```

Slices B, C, D can all be parallelized after A. Slice F must wait for Slice E's v0.2-9 gate. Slice G is technically independent of all enrichment slices but depends on an external system (HIR extractor output). SchemaVersion bumps for B, C, D must be coordinated — if they land in separate PRs each needs its own bump; if they land together a single v0.2.1 bump covers all three.

---

## Blockers to RATIFY

1. **RFC amendment required before Slice A can start.** The rename of `enrich_docs` → `enrich_rfc_docs` narrows the published Language surface; the addition of the sixth `enrich_concepts` pass extends the RFC §A2.2 table. Both must be explicitly recorded in the RFC with an RFC status update (not just as implementation choices).

2. **RFC §A2.2 does not name a SchemaVersion bump.** The four new attribute groups (git_history, rfc_docs, deprecation, reachability) each require at minimum a patch bump. The RFC must name the target version(s) and coordinate the lockstep graph-specs-rust PRs.

3. **`enrich_bounded_context` schema already populated at extraction time.** This is a partial duplicate concern — the RFC implies this is new work, but `cfdb-extractor/src/lib.rs:105–142` already emits it. The RFC must clarify whether Slice E is an extraction-time guarantee (the pass is a no-op) or a genuine re-enrichment (the pass re-reads TOML and patches the graph). Implementing it as a genuine re-enrichment without this clarification risks duplicating the extraction logic.

4. **The RFC §A2.2 classifier Cypher has no confidence-gating for the homonym class.** As detailed in Q3, the classifier fires `context_homonym` on a hard boolean. The v0.2-9 accuracy gate (≥95%) is the only safeguard. The RFC must explicitly state that the two-stage classifier (Stage 2 Cypher) must not be deployed until v0.2-9 passes — this dependency must be a named invariant in the RFC, not an implicit assumption.

5. **`enrich_deprecation` provenance ambiguity.** The RFC §A2.2 says "no new I/O; reuses existing AST walk" — this implies extraction time, not enrichment time. If it is extraction time, the Phase D stub and enrichment pass are wrong; it should be an extractor extension. The RFC must resolve this before Slice C begins.

---

## References

- `/var/mnt/workspaces/cfdb/.claude/worktrees/43-enrichment/crates/cfdb-core/src/enrich.rs` (lines 80–111) — current four stubs
- `/var/mnt/workspaces/cfdb/.claude/worktrees/43-enrichment/crates/cfdb-core/src/schema/labels.rs` (lines 17–115) — node/edge label constants
- `/var/mnt/workspaces/cfdb/.claude/worktrees/43-enrichment/crates/cfdb-core/src/schema/describe.rs` (lines 24–315) — runtime schema descriptor
- `/var/mnt/workspaces/cfdb/.claude/worktrees/43-enrichment/crates/cfdb-core/src/schema/descriptors.rs` (lines 24–44) — Provenance enum
- `/var/mnt/workspaces/cfdb/.claude/worktrees/43-enrichment/crates/cfdb-concepts/src/lib.rs` (lines 1–196) — shared bounded-context resolver
- `/var/mnt/workspaces/cfdb/.claude/worktrees/43-enrichment/crates/cfdb-extractor/src/lib.rs` (lines 81–194) — extraction including context emission
- `/var/mnt/workspaces/cfdb/.claude/worktrees/43-enrichment/crates/cfdb-cli/src/enrich.rs` — CLI dispatch
- `/var/mnt/workspaces/cfdb/.claude/worktrees/43-enrichment/crates/cfdb-cli/src/main.rs` (lines 119–147) — CLI command definitions
- `/var/mnt/workspaces/cfdb/.claude/worktrees/43-enrichment/crates/cfdb-query/src/inventory.rs` — `reachable_from_entry_point` in query layer
- `/var/mnt/workspaces/cfdb/.claude/worktrees/43-enrichment/docs/RFC-cfdb-v0.2-addendum-draft.md` (lines 171–306) — §A2 + §A3 classifier pipeline

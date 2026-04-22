# Code Facts Database — v2 plan

**Status:** pre-RFC substrate. Intended as the input for the next ratified RFC (working title: RFC-036 — cfdb v2: closing HSB, VSB, and raid validation).
**Date:** 2026-04-22.
**Authoritative predecessors:** `docs/RFC-cfdb.md` (v0.1 ratified and shipped), `docs/RFC-033-cross-dogfood.md`, `docs/RFC-034-query-dsl.md`, `docs/RFC-035-persistent-inverted-indexes.md` (in progress, slices 1–6/7 merged).
**Historical substrate:** `docs/PLAN-v1-code-facts-database.md` (trimmed to the timeless material).

This plan does not restart cfdb. It records the pivot taken between PLAN-v1 and the shipped v0.1, then scopes what remains to deliver on the three original motivating problems: **horizontal split-brain (HSB), vertical split-brain (VSB), bounded-context raid plan validation.**

---

## 1. What shipped in v0.1 (the pivot)

PLAN-v1 framed cfdb around three motivating problems and four consumer classes. v0.1 delivered the substrate but chose a different first headline. The pivot, concisely:

| PLAN-v1 framing | v0.1 reality |
|---|---|
| In-tree tooling, `.concept-graph/`-adjacent | Standalone repo (`yg/cfdb`), paired with `yg/graph-specs-rust` under the RFC-033 cross-dogfood contract |
| Remote graph server (one server, many clients) | Library-first: `StoreBackend` trait in `cfdb-core`; in-process `cfdb-petgraph` backend; CLI and HTTP as equal peer wire forms |
| HTTP primary consumer interface | Rust lib as primary; graph-specs-rust vendors cfdb as a pinned git dep |
| `syn` first, escalate to `ra-ap` if blocked | Both coexist: `cfdb-extractor` (syn, v0.1) + `cfdb-hir-extractor` (HIR, v0.2, feature-gated). `:CallSite.resolver` discriminator (`"syn"` vs `"hir"`) partitions the two |
| First consumer: `/prescribe` grounding **or** `/gate-raid-plan` | First consumer: **ban rules** (`.cfdb/queries/*.cypher` run in CI). Headline AC was Pattern D (`arch-ban-utc-now.cypher` equivalent to the handwritten Rust architecture test) |
| Cypher as the only query language | Cypher subset + Rust fluent builder DSL (RFC-034) |
| Performance scale: "add indices if needed" | Real 148k-node keyspace hit a 16+ min wall; RFC-035 (persistent inverted indexes, slices 1–6/7 merged) is paying it down |

### 1.1 Why the pivot

Three forces moved cfdb toward ban rules as the first felt win:

- **Smallest AC surface that stressed the full pipeline.** One `.cypher` file replacing one handwritten architecture test exercised extractor → store → query → CI integration end-to-end. HSB/VSB/raid would each have required a larger schema commitment upfront.
- **Graph-specs-rust needed a preventive counterpart.** cfdb as retrospective X-ray + graph-specs as preventive vaccine is a stronger product than cfdb alone. The cross-dogfood contract (RFC-033) gave both sides a shared zero-false-positive guarantee that neither would have had as a single tool.
- **Performance surprises pulled scope.** HSB cluster queries and raid joins sit on the wrong side of the 148k-node performance cliff without persistent indexes. RFC-035 had to ship first.

### 1.2 What the pivot cost

- **HSB clustering (PLAN-v1 §7 row 3) — not shipped.** Same-name HSB is expressible; multi-signal clustering is not.
- **VSB detector (PLAN-v1 §7 row 4) — not shipped.** Call graph exists in `cfdb-hir-extractor`; `:EntryPoint` discovery is not implemented (the node type has no producer).
- **Raid plan validation (PLAN-v1 §7 rows 21–25) — not shipped.** Quality signals are not on `:Item` nodes; `query_with_input()` exists but is untested on raid-shaped inputs.

Those are the three gaps v2 closes.

---

## 2. What's still unsolved

### 2.1 Horizontal split-brain (HSB) clustering

**Gap:** three of the four multi-signal signals are unavailable.

- `signature_hash` on `:Item` — present for `Fn` kind, needs audit for `Struct`/`Enum`/`Trait`.
- Neighbor-set materialization (or graph-native Jaccard query) — not implemented.
- `conversion_targets` — per-`:Item` set of `:Item` types it converts to/from via `From`/`Into`/`TryFrom` impls. Derivable from existing `IMPLEMENTS_FOR` edges; needs a query or enrichment.
- Cluster emitter — transitive grouping so consumers see "three impls of one concept," not "three pairs."

### 2.2 Vertical split-brain (VSB) detection

**Gap:** entry-point catalog is empty.

- `:EntryPoint` discovery for the three mechanisms in this workspace: MCP tool registrations, clap derive macros, axum routes. (cron is a v3 concern.)
- `EXPOSES` edges from `:EntryPoint` to handler `:Item`.
- `REGISTERS_PARAM` edges from `:EntryPoint` to `:Param`.
- The `vertical` query itself: BFS `CALLS*` from handler, count distinct `RETURNS` of the entry-param type, flag count > 1.
- Scar corpus: concrete entry-point/param pairs drawn from known guardrail failures (Param-Effect Canary, MCP Boundary Fix AC, compound stop layer isolation) against which detector precision can be measured.

### 2.3 Raid plan validation

**Gap:** quality signals are in parallel tools (clippy, cargo-llvm-cov, audit-split-brain); the "one fact base" premise requires them on `:Item`.

- Quality attributes on `:Item`: `unwrap_count`, `cyclomatic`, `test_coverage`, `dup_cluster_id`.
- Plan YAML schema — portage / rewrite / glue / drop buckets with item qnames.
- The five plan-validation queries: completeness, dangling-drop, hidden-callers, missing-canonical, signal-mismatch.
- `query_with_input()` coverage for raid-shaped inputs (named sets from plan.yaml).
- `/gate-raid-plan` skill (consumer-side, lives outside cfdb).

---

## 3. v2 scope

v2 ships the minimum schema, query, and enrichment surface needed to close §2's three gaps — in the order that produces felt wins soonest and stresses the weakest part of the pipeline first.

### 3.1 Sequencing

1. **EntryPoint discovery.** Prerequisite for VSB. Unlocks a new ban-rule shape ("every MCP handler must delegate to a domain `FromStr`"). Smallest schema delta.
2. **VSB detector.** Uses HIR call graph (already shipped) + the new EntryPoint nodes. Each detector hit maps to a known guardrail scar, so precision is measurable day one.
3. **Quality enrichment pass.** Adds `unwrap_count`, `cyclomatic`, `test_coverage`, `dup_cluster_id` on `:Item`. Unlocks HSB row 3, row 25 (raid signal-mismatch), and row 18 (cyclomatic hot spots) simultaneously.
4. **HSB clustering.** Requires the enrichment pass + RFC-035 indexes on `signature_hash`.
5. **Raid plan validation.** Requires all of the above + plan.yaml schema.

Each step ships behind a feature flag until its scar corpus is green on the target workspace.

### 3.2 Schema deltas

All additive, within the existing `SchemaVersion` contract (minor bump per step). Every delta ships with a lockstep PR on graph-specs-rust per the CLAUDE.md §3 rule.

- **Step 1:** `:EntryPoint` producers (MCP / clap / axum); `EXPOSES`, `REGISTERS_PARAM` edges.
- **Step 3:** `unwrap_count`, `cyclomatic`, `test_coverage`, `dup_cluster_id`, `signature_hash` (audit for non-Fn kinds), `conversion_targets` on `:Item`.

### 3.3 Verb deltas

**None.** The 11-verb API (PLAN-v1 §6.1) is sufficient. v2 ships additional Cypher files + one enrichment pass under the existing `enrich_metrics` verb. Keeping the verb count at 11 is a load-bearing constraint — any pressure to add a 12th should surface here and get pushed back into schema or composition.

### 3.4 Wire-form deltas

None. CLI + HTTP + Rust lib remain the three wire forms. `query_with_input()` wire-up over HTTP gets exercised for the first time by the raid validation step (step 5).

### 3.5 Acceptance, per problem

**HSB clustering:**
- Multi-signal cluster query runs in < 10s on a target workspace at v0.1 shipped scale (~15k `:Item` nodes), under the RFC-035 indexes.
- Spot-check: 10 randomly-sampled clusters, ≥ 7 are real candidates.
- Catches every same-name HSB a regex audit flags on the target workspace, plus at least one synonym-renamed duplicate that name-match cannot see.
- Dogfood: run on cfdb's own source tree; zero clusters, or every cluster is triaged and either fixed or documented in `KNOWN_GAPS.md`.

**VSB:**
- `vertical` query finds at least one real candidate per known guardrail scar (Param-Effect Canary, MCP Boundary Fix AC, compound stop layer isolation) on the target workspace.
- For each flagged entry point, the `provenance` query returns a full call-chain trace with each resolver annotated.
- Zero flags on a clean scar-free fixture (synthetic workspace with single-resolver entry points).
- Dogfood: every `.cfdb/queries/` file that is itself a resolver entry point shows exactly one resolver per param (cfdb's own entry points are clean by construction).

**Raid:**
- Given a hand-authored plan.yaml for one real bounded context, the five plan-validation queries return a pass/fail + a list of specific holes.
- When the plan is deliberately incomplete (one dropped `:Item` still referenced from the portage set), the dangling-drop query flags the exact referencing `:Item`.
- Validation runs in < 30s on a target workspace graph.
- End-to-end demo: a plan flagged by v2, fixed by the plan author, re-validated green, then executed — the raid lands without orphan or dangling-ref discovery during the raid itself.

Each acceptance includes a `Tests:` block per CLAUDE.md §2.5: Unit + Self dogfood + Cross dogfood (graph-specs-rust) + Target dogfood.

### 3.6 Out of scope for v2

- LLM enrichment (still deferred — waiting for a consumer use case that requires it).
- Multi-language extraction.
- IDE integration.
- Auto-fix / code modification.
- A `/gate-raid-plan` skill *implementation* — v2 delivers the queries and the plan schema; the skill lives in the consumer workspace.
- UI / dashboard for HSB clusters.
- Incremental re-extraction (re-extract from scratch remains the model; RFC-035 makes this cheap enough).

---

## 4. Risks

1. **Entry-point discovery is workspace-specific.** A workspace that registers MCP tools via a custom macro or a non-standard HTTP framework needs a custom detector. **Mitigation:** document the three supported mechanisms up front; require new mechanisms to ship with a detector + scar test in the same PR.
2. **Quality enrichment cost at 148k-node scale.** Cyclomatic complexity and `unwrap_count` require re-walking every function body; `test_coverage` requires mapping test items to their targets. **Mitigation:** make the enrichment pass incremental against a changed-files set; only re-compute attributes on `:Item` nodes whose `signature_hash` changed since the last pass.
3. **HSB cluster precision depends on normalization.** Two items with identical structural hash but different concepts (e.g. every `struct Marker;` in the workspace) would cluster falsely. **Mitigation:** exclude unit structs and empty types from structural-hash matching; document the carve-out in the schema.
4. **Quality attributes double the schema surface to track for determinism.** `unwrap_count` and `cyclomatic` must be byte-stable across re-extracts. **Mitigation:** compute them as pure functions of the AST in a dedicated Layer 2 pass; same determinism CI check covers them.
5. **Raid validation assumes plan.yaml author cooperation.** Plans that silently omit items evade completeness checks unless the source context is declared. **Mitigation:** the completeness query takes the source crate explicitly, not inferred from the plan; the plan schema requires a `source_context` field.
6. **Cross-dogfood lockstep tax.** Every schema delta in v2 is a lockstep PR on graph-specs-rust (CLAUDE.md §3). Five steps means five lockstep dances. **Mitigation:** batch step 1 + step 3's schema additions into a single `SchemaVersion` minor bump if sequencing allows; otherwise accept the tax as the price of the contract.

---

## 5. Open questions (council)

1. **Scar corpus for VSB.** Where do the concrete entry-point/param pairs come from? Hand-authored fixture in cfdb's own tests, or a pinned SHA of a target workspace known to contain scars? Trade-off: fixture is fully controlled but synthetic; pinned SHA is realistic but couples cfdb's CI to a downstream repo's history.
2. **Quality attribute provenance.** Are `unwrap_count` and `cyclomatic` computed by cfdb directly (visiting the AST in a Layer 2 pass), or consumed from an external tool (`cargo-geiger`, a custom complexity counter)? Trade-off: in-process keeps the determinism guarantee; external offloads the maintenance cost.
3. **Plan.yaml schema ownership.** Does cfdb define the plan schema (it becomes part of the v1.x contract), or does the consumer workspace define it (cfdb just needs `query_with_input()` to accept arbitrary named sets)? The second is simpler; the first is safer against consumer-side drift.
4. **HSB cluster output shape.** Flat list of `(cluster_id, item_qnames[], signals_matched[])`, or a richer structured report (per-cluster summary + representative item + quality deltas)? Flat is simpler; rich is more useful for agent consumers that want to pick a canonical.
5. **Sequencing override.** Should step 3 (quality enrichment) come before step 2 (VSB detector)? Step 2 as currently sequenced uses only structural facts, so it can ship first — but if the quality pass is easy and its acceptance is cheap, running step 3 first unlocks HSB, raid signal-mismatch, and cyclomatic hot spots in one go. Council decides based on implementation-cost estimate.

---

## 6. References

- `docs/PLAN-v1-code-facts-database.md` — original plan (trimmed, historical)
- `docs/RFC-cfdb.md` — v0.1 ratified RFC
- `docs/RFC-033-cross-dogfood.md` — companion-tool contract
- `docs/RFC-034-query-dsl.md` — Rust fluent query builder
- `docs/RFC-035-persistent-inverted-indexes.md` — scaling work (slices 1–6/7 merged at time of writing)
- `CLAUDE.md` §2 — RFC pipeline + architect team review
- `CLAUDE.md` §3 — dogfood enforcement + SchemaVersion lockstep rule

---

**End of plan.** Substrate for the next RFC — proposed title "RFC-036 — cfdb v2: closing HSB, VSB, and raid validation." Council convene when ready.

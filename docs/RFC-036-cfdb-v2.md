---
title: RFC-036 — cfdb v2 (closing HSB, VSB, raid validation)
status: Draft — R1 council complete, pending R2 ratification
date: 2026-04-23
supersedes: none (extends RFC-cfdb v0.1)
preceded_by: RFC-cfdb, RFC-032, RFC-033, RFC-034, RFC-035
substrate: docs/PLAN-v2-solving-original-problems.md
council_team: council-036-cfdb-v2
tracking_issue: (to be filed after ratification)
---

# RFC-036 — cfdb v2: closing HSB, VSB, and raid validation

## 1. Problem

cfdb v0.1 shipped a deterministic fact base, a library-first API, the ban-rule headline (Pattern D — `arch-ban-utc-now.cypher` replacing a handwritten Rust architecture test), and a cross-dogfood contract with `graph-specs-rust` (RFC-033). It also shipped persistent inverted indexes (RFC-035, slices 1–6/7 merged) that bring the 148k-node keyspace down from a 16+ minute scope wall to sub-second queries.

**What v0.1 did not ship** are the three problems PLAN-v1 was written to solve:

- **Horizontal split-brain (HSB) clustering.** Same-name duplicates are expressible in the v0.1 Cypher subset. Multi-signal clustering (structural hash + neighbor Jaccard + normalized name + conversion-target sharing) is not — three of the four signals require attributes or query patterns the schema does not yet carry.
- **Vertical split-brain (VSB) detection.** The HIR-resolved call graph ships in `cfdb-hir-extractor` (RFC-032, v0.2 feature-gated). `:EntryPoint` + `EXPOSES` emission shipped via issues #86 / #124 / #125. `REGISTERS_PARAM` is declared in the schema (`EdgeLabel::REGISTERS_PARAM`) but has **no producer** — the entry-point catalog lacks its parameter set, so the detector cannot BFS from "an MCP tool's `tf` parameter" to "all reachable resolvers of `Timeframe`."
- **Bounded-context raid validation.** Quality signals (`unwrap_count`, `cyclomatic`, `test_coverage`, `dup_cluster_id`) live in parallel tools (`clippy`, `cargo-llvm-cov`, `audit-split-brain`). PLAN-v1 §2.3 argued these must live on `:Item` as attributes, or the raid-plan-validation query must join two data stores — defeating the "one fact base" premise.

These three gaps are the scope of v2. The v0.1 pivot to ban-rules-first bought always-on per-PR enforcement and the graph-specs companion; v2 returns to the original motivating problems without backing off any v0.1 guarantee.

## 2. Scope

v2 ships **five additive steps** that close the three gaps. Every step is internally feature-gated where appropriate; every schema delta is additive under the existing `SchemaVersion` contract.

| Step | Deliverable | Closes |
|---|---|---|
| 1 | `REGISTERS_PARAM` emission extending `cfdb-hir-extractor::entry_point_emitter` + scar corpus for MCP `#[tool]` + clap `#[arg]` param shapes | VSB prerequisite |
| 2 | `vertical` query (Cypher composition) using the HIR `CALLS` graph + `:EntryPoint -[:EXPOSES]-> :Item -[:CALLS*]-> :Item{kind:Fn}` reachability + `RETURNS`-type multiplicity check | VSB |
| 3 | `enrich_metrics` real implementation in `cfdb-petgraph` (behind `quality-metrics` feature): `unwrap_count`, `cyclomatic`, `test_coverage` (sub-feature `llvm-cov`), `dup_cluster_id` attributes on `:Item` | HSB prerequisite + raid prerequisite |
| 4 | HSB multi-signal cluster query (Cypher composition) using RFC-035 indexes over `signature_hash` + `name` + conversion-target sharing | HSB |
| 5 | Five raid-plan-validation Cypher templates consumed via `query_with_input()` with plan.yaml bucket names as external sets | Raid validation |

**Zero verb deltas.** The 11-verb API holds. Every step composes over `extract`, `enrich_metrics`, `query`, `query_with_input`.

**Scope correction discovered in council R1:** `:EntryPoint` + `EXPOSES` emission already shipped (issues #86 / #124 / #125 per `cfdb-hir-extractor/src/entry_point_emitter.rs`). PLAN-v2 §3.1 step 1 is **reduced** to REGISTERS_PARAM + `:Param` producer + scar corpus.

Out of scope (see §6): LLM enrichment, multi-language extraction, IDE integration, auto-fix, plan.yaml parser (consumer-side), `/gate-raid-plan` skill implementation (consumer-side).

## 3. Design

### 3.1 Step 1 — `REGISTERS_PARAM` emission

**Schema deltas (additive; `SchemaVersion` v0.3.0):**

- `EdgeLabel::REGISTERS_PARAM` already declared in `cfdb-core/src/schema/labels.rs:101`; v2 adds its producer.
- **`:Param` node label** already exists as `Label::PARAM` (`cfdb-core/src/schema/labels.rs:24`) and has a descriptor in `schema/describe/nodes.rs:223-262`. v2 uses **the same `:Param` nodes emitted by `HAS_PARAM`** as the target of `REGISTERS_PARAM`. Node identity is shared via the existing qname id formula in `cfdb-core::qname`; edge semantics are distinct (HAS_PARAM = "this function declares this parameter"; REGISTERS_PARAM = "this entry point exposes this parameter to an external caller"). **No new node label; no id formula divergence.** (Convergence Point CP1 — resolves clean-arch B1 + ddd cond 2 + rust-systems P-RS1.)

**Ownership:** extend `cfdb-hir-extractor::entry_point_emitter`. No new crate (clean-arch: a new crate adds a dependency edge without adding architectural clarity; solid: a separate `cfdb-entrypoint-extractor` would have I≈0.67 vs cfdb-extractor's I=0.50 — CCP/CRP argue against the split). `:Param` nodes continue to be produced by the existing `HAS_PARAM` emitter in `cfdb-extractor` — the HIR extractor reuses them by qname lookup.

**Detection contracts (rust-systems):**

- **MCP `#[tool]` fn:** parse `syn::ItemFn::sig.inputs` syntactically. Each non-self param → one `REGISTERS_PARAM` edge to the `:Param` node with matching `(parent_qname, name, index)`. Names and type paths are available without HIR resolution.
- **Clap `#[derive(Parser)]` struct / `#[derive(Subcommand)]` variant:** walk struct fields for `#[arg(...)]` attributes. Field name = param name; field type path = param type. Each `#[arg(...)]`-annotated field → one `REGISTERS_PARAM` edge.
- **Axum `Router::route("/path", method(handler))` handler:** deferred to v3. Handler params require HIR fn-signature resolution of the extracted function; v2 emits `http_route` `:EntryPoint` nodes with empty `params` list (documented explicitly in `SchemaDescribe` output as "not yet supported").

**`:EntryPoint` lifecycle (CP4 — resolves ddd cond 1):** `:EntryPoint` is a **value object**, not an aggregate root. It exists if and only if its handler `:Item{kind:Fn}` exists. Re-extraction of a keyspace where the handler has been deleted must remove the associated `:EntryPoint` and all its outgoing `EXPOSES` / `REGISTERS_PARAM` edges in the same transaction. The extractor emits `:EntryPoint` as a first-class node (for query ergonomics — "BFS from all entry points") but the lifecycle constraint is documented as an invariant.

### 3.2 Step 2 — VSB detector

**No schema delta.** Cypher composition over the shipped HIR call graph + the step-1 `:EntryPoint` + `REGISTERS_PARAM` edges.

**Query shape** (`.cfdb/queries/vsb-multi-resolver.cypher`):

```cypher
MATCH (e:EntryPoint)-[:EXPOSES]->(h:Item)
MATCH (e)-[:REGISTERS_PARAM]->(p:Param)
MATCH (h)-[:CALLS*1..10]->(f:Item {kind: 'Fn'})
WHERE (f)-[:RETURNS]->(t:Item) AND t.qname = p.type_normalized
WITH e, p, collect(DISTINCT f.qname) AS resolvers
WHERE size(resolvers) > 1
RETURN e.name, p.name, resolvers
```

**Scar corpus (rust-systems prescription):** fixtures drawn from known guardrail failures — Param-Effect Canary, MCP Boundary Fix AC Template, compound stop layer isolation. Each fixture is a synthetic two-crate workspace (one correct single-resolver entry point + one scar-shaped multi-resolver entry point) committed under `crates/cfdb-extractor/tests/fixtures/vsb/`.

### 3.3 Step 3 — Quality enrichment

**Schema deltas (additive; `SchemaVersion` v0.3.1):**

Four `:Item` attributes with `Provenance::EnrichMetrics`:

- `unwrap_count: usize` — count of `.unwrap()` and `.expect()` expressions in the function body. `0` for non-Fn items.
- `cyclomatic: usize` — cyclomatic complexity via standard McCabe counting on the AST (branches + 1). `0` for non-Fn items.
- `test_coverage: Option<f64>` — line coverage ratio from `cargo-llvm-cov` JSON output, if available; `None` otherwise. **Excluded from G1** (CP2 / §4 invariant G6).
- `dup_cluster_id: Option<String>` — sha256 of lex-sorted newline-joined member qnames of the HSB cluster this item belongs to, if any (CP5).

**`enrich_metrics` is stateless** — no `changed_files` parameter, no `EnrichBackend` trait change. Full re-walk per invocation. If step-3 cost at 148k-node scale is unacceptable, an incremental re-walk is a future RFC, not a v2 trait change. (CP3 — resolves clean-arch B2.)

**Internal decomposition (SOLID SRP prescription):**

```
cfdb-petgraph/src/enrich/metrics/
├── mod.rs         — pass coordinator; impl EnrichBackend::enrich_metrics
├── ast_signals.rs — unwrap_count + cyclomatic (pure functions, zero I/O)
├── coverage.rs    — test_coverage (I/O boundary: reads cargo-llvm-cov JSON)
└── clustering.rs  — dup_cluster_id (cross-item HSB cluster assignment)
```

**DIP constraint (SOLID P1):** `ast_signals.rs` invokes `syn::parse_file` directly from within `cfdb-petgraph`. `cfdb-petgraph → cfdb-extractor` is forbidden. `syn` enters `cfdb-petgraph/Cargo.toml` as an optional dep under the `quality-metrics` feature (never unconditional).

**Feature-flag layout (rust-systems prescription):**

```toml
# cfdb-petgraph/Cargo.toml
[features]
quality-metrics = ["dep:syn"]      # enables enrich_metrics real implementation
llvm-cov = ["quality-metrics"]     # enables test_coverage sub-feature
```

**Parallelism:** `ast_signals.rs` uses rayon with sort-before-emit. G1 remains satisfied because emission order is deterministic. Documented as an invariant clause in `cfdb-petgraph.md` spec (see §5 spec amendments).

### 3.4 Step 4 — HSB multi-signal cluster query

**No schema delta** (step 3 shipped the attributes). Cypher composition over RFC-035 indexes.

**Query shape** (`.cfdb/queries/hsb-cluster.cypher`): four signals joined with ≥2-signal acceptance threshold:

1. Same `signature_hash` (RFC-035-indexed).
2. Same normalized name — exact match or edit-distance-1 (the Cypher subset does not currently support ed1; v2.1 extension point).
3. Neighbor-set Jaccard ≥ 0.6 over `TYPE_OF` neighbours.
4. Shared conversion target — both items implement `From<T>` / `Into<T>` / `TryFrom<T>` for the same `T`.

`dup_cluster_id` is the sha256 of the lex-sorted member qname set; populated by step-3 `clustering.rs`.

Unit structs and empty types are excluded from structural-hash matching to avoid false clustering (every `struct Marker;` would otherwise cluster). Carve-out documented in `cfdb-core.md`.

### 3.5 Step 5 — Raid plan validation

**No schema delta.** Five Cypher templates shipped under `examples/queries/raid/`. Consumed via `query_with_input()` with plan.yaml bucket names as external named sets.

**Plan.yaml schema is NOT defined by cfdb.** (CP7 — resolves solid P2.) The consumer workspace owns the YAML parser; cfdb ships only the query templates. Required named sets: `portage`, `rewrite`, `glue`, `drop`. Required scalar: `source_context` (the crate being raided, so the completeness query doesn't have to infer it from the plan).

**Queries:**

1. `raid-completeness.cypher` — every `:Item` in `source_context` not named in any bucket.
2. `raid-dangling-drop.cypher` — items in `drop` with incoming CALLS / TYPE_OF from `portage` + `glue`.
3. `raid-hidden-callers.cypher` — items in `portage` with incoming edges from outside `source_context`.
4. `raid-missing-canonical.cypher` — rewrite-bucket concepts without a target `:Item` carrying `CANONICAL_FOR`.
5. `raid-signal-mismatch.cypher` — items tagged "portage (clean)" whose `unwrap_count` / `test_coverage` / `dup_cluster_id` contradict the "clean" claim.

## 4. Invariants

**Existing (unchanged from RFC-cfdb / RFC-029):**

- **G1 — byte-stable extraction.** Same `(workspace SHA, schema major.minor)` → byte-identical canonical JSONL dump.
- **G2 — read-only queries.** `query()` and `query_with_input()` never mutate the graph.
- **G3 — additive enrichments.** `enrich_*()` never deletes structural facts.
- **G4 — monotonic SchemaVersion within a major.** v0.3 graphs are queryable by v0.3.x consumers and any future higher-(minor, patch) reader within the 0.x major. A v0.1.x or v0.2.x reader at a lower (minor, patch) correctly refuses v0.3.0 graphs per the `SchemaVersion::can_read` rule (`crates/cfdb-core/src/schema/labels.rs:312-315`: `self.major == graph.major && graph <= self`) — this is the intended forward-incompatibility signal when v0.3 introduces new node types older readers cannot handle. Bumps inside a major are additive-only.
- **G5 — immutable snapshots.** Keyspaces are never rewritten in place; dropped or replaced wholesale.

**New — G6 (CP2; resolves solid P1 + rust-systems P-RS2):**

> **G6 — toolchain-scoped attributes.** `test_coverage` is byte-stable only within the same Rust toolchain version. It is **excluded from the G1 canonical-dump sha256** and declared as toolchain-version-scoped in `SchemaDescribe` output. Callers may record the toolchain version alongside the keyspace if they need cross-toolchain comparability; the tool does not record it automatically. Any future attribute with similar scoping must be declared under G6 at introduction.

G6 is additive; no existing guarantee breaks.

**CP5 — `dup_cluster_id` determinism:** the cluster id is `sha256(lex_sorted(member_qnames).join("\n"))`. Deterministic across re-extracts; insensitive to extraction order. Assignment happens in step-3 `clustering.rs` after all `signature_hash` attributes are populated.

**Recall:** no change from RFC-cfdb (≥ 95% per crate against `cargo public-api` / `rustdoc --output-format json` ground truth).

**No-ratchet:** no change from CLAUDE.md §3. Every threshold in tool source stays a `const`; no baseline / ceiling / allowlist files introduced by v2.

**Keyspace backward-compat:** v0.1 and v0.2 keyspaces remain queryable by v0.3 consumers for all structural facts. Quality attributes and `REGISTERS_PARAM` are absent in older keyspaces; queries that require them return empty results, not errors. `SchemaDescribe` output reports the keyspace's own schema version.

## 5. Architect lenses

### Clean Architecture lens

**Lens:** Robert C. Martin's Clean Architecture — dependency rule, port purity, composition root.
**Round:** R1
**Verdict:** REQUEST CHANGES (two blocking items; resolved in §3.1 and §3.3 of this RFC)

#### Layer-boundary compliance

The existing crate DAG is acyclic and correctly implements the dependency rule. Verified from `Cargo.toml` across all nine crates:

```
Level 0 (innermost): cfdb-core, cfdb-concepts
Level 1:             cfdb-query, cfdb-extractor, cfdb-hir-extractor
Level 2:             cfdb-petgraph, cfdb-recall
Level 3:             cfdb-hir-petgraph-adapter
Level 4 (outermost): cfdb-cli
```

All arrows point inward. No cycle exists. v2's five steps do not introduce new crate dependency edges — `:EntryPoint` emission stays in `cfdb-hir-extractor` (already shipped); `enrich_metrics` implements in `cfdb-petgraph` (existing stub); VSB/HSB/raid ship as Cypher files and `Param` bindings, not as new crates.

#### StoreBackend and EnrichBackend port purity

`StoreBackend` (`crates/cfdb-core/src/store.rs:62-87`) takes only `cfdb_core` types in every method signature: `Keyspace`, `Node`, `Edge`, `Query`, `QueryResult`, `StoreError`. Zero sqlx, tokio, or reqwest types. Unchanged by v2.

`EnrichBackend` (`crates/cfdb-core/src/enrich.rs:91-192`) takes only `&Keyspace`, returns `Result<EnrichReport, StoreError>`. The `enrich_metrics` method (`enrich.rs:189`) already exists as a Phase A stub. v2 step 3 adds the real implementation in `cfdb-petgraph::PetgraphStore` — no port surface change required.

**Port purity verdict: INTACT.** The four quality attributes are already declared as descriptors in `cfdb-core/src/schema/describe/nodes.rs:123-130` under `Provenance::EnrichMetrics`. No new schema types enter the port.

#### BLOCKING ITEMS (resolved in this RFC)

- **B1 — REGISTERS_PARAM emission ownership.** Resolved in §3.1: `cfdb-hir-extractor::entry_point_emitter` owns REGISTERS_PARAM emission; `:Param` nodes are the existing ones produced by `HAS_PARAM`; detection contracts spelled out for clap `#[arg(...)]` and MCP `#[tool]` shapes.
- **B2 — enrich_metrics incremental strategy.** Resolved in §3.3 and §4: stateless full re-walk, no `changed_files` parameter, no `EnrichBackend` trait change.

#### Peer challenges

- **To DDD:** `:EntryPoint` aggregate-vs-decorator. Clean-arch endorses first-class node for `REGISTERS_PARAM` attachment clarity. DDD resolved: value object tied to handler `:Item` lifecycle — compatible with clean-arch's position.
- **To SOLID:** `enrich_metrics` SRP splitting. Clean-arch position: internal decomposition, not verb split. SOLID resolved with 3-module decomposition — aligned.

### Domain-Driven Design lens

**Lens:** DDD aggregates / value objects / bounded contexts / ubiquitous language.
**Round:** R1
**Verdict:** RATIFY WITH 3 CONDITIONS (all three resolved in §3 and §4 of this RFC)

#### Bounded-context separation

cfdb's domain (structural facts about Rust code) and the consumer workspace's domain (target-workspace concepts) remain disjoint. `:Concept` is an overlay on `:Item`, not a conflation. Plan.yaml vocabulary (portage/rewrite/glue/drop) stays outside cfdb (§3.5, §6). **RATIFY.**

#### `:EntryPoint` classification — value object, not aggregate root

`:EntryPoint` holds `kind`, `name`, `handler_qname`, `params`. Its identity is derived from the handler `:Item{kind:Fn}`; it has no independent lifecycle. First-class node for query ergonomics (BFS starting vertex) — correct. Aggregate-root classification would have imposed independent lifecycle semantics that the domain does not have.

**Condition 1 resolved** in §3.1: lifecycle constraint documented — no standalone `:EntryPoint` without a corresponding handler `:Item`.

#### Quality attributes on `:Item` — RATIFY

Structural kinds (what the parser saw) stay on `:Item`; labels (what we infer) stay in the overlay. Quality attributes are a third category: derived signals, deterministic within toolchain scope (G6). Placing them on `:Item` with `Provenance::EnrichMetrics` keeps the join-free raid query premise intact. A `:Metric` node split would violate REP (no independent reuse case) — SOLID-side rebuttal is accepted. **RATIFY.**

#### Homonym audit — `REGISTERS_PARAM` / `HAS_PARAM`

The same word "Param" names two concepts in the codebase: query-AST `Param` (`cfdb-core::query::ast::Param`) and the `:Param` graph node label. Spec amendment renames the query-AST type to `ParamBinding` to eliminate ambiguity.

For the edge semantics: `HAS_PARAM` (function declares parameter) and `REGISTERS_PARAM` (entry point exposes parameter to caller) both target the **same** `:Param` nodes with the same id formula. Distinct edges, distinct semantics, shared node identity. **Condition 2 resolved** in §3.1.

#### `dup_cluster_id` determinism

SHA256 of lex-sorted newline-joined member qnames. Deterministic across re-extracts; insensitive to iteration order; satisfies G1 (unlike `test_coverage`, which requires G6). **Condition 3 resolved** in §4.

#### `EXPOSES` not a homonym

`EXPOSES` (EntryPoint → handler Item) is semantically distinct from `CALLS` (Fn → Fn). EXPOSES marks the external-surface boundary; CALLS is an internal dispatch edge. No vocabulary collision. **RATIFY.**

#### Peer challenges

- **To clean-arch:** entry-point detection is a Layer 1 structural fact; separate crate adds fragility without DDD benefit. Clean-arch aligned — extend existing HIR extractor.
- **To SOLID:** closed `kind` enum on `:EntryPoint` with documented minor-bump extension semantics satisfies OCP. SOLID aligned — string discriminator + additive extension policy.

### SOLID + component principles lens

**Reviewer:** solid-architect | **Verdict:** RATIFY with two prescriptions (both resolved in §3 / §4 / §6 of this RFC)

#### ISP — 11-verb API holds under v2 pressure

PLAN-v2 §3.3 claims zero verb deltas. All five steps were examined:

- **Step 1 (EntryPoint discovery):** schema delta only, extraction happens inside `extract()`. No verb pressure.
- **Step 2 (VSB detector):** Cypher composition via `query()`. No verb pressure.
- **Step 3 (quality enrichment):** `enrich_metrics` already exists as a stub in `EnrichBackend`. The single-verb approach is ISP-correct — no consumer wants fewer than all four quality signals (raid signal-mismatch requires all four). Splitting would violate CRP without providing ISP benefit.
- **Step 4 (HSB clustering):** Cypher composition over RFC-035 indexes via `query()`. No verb pressure.
- **Step 5 (raid plan validation):** five Cypher queries via `query_with_input()` with plan buckets as named sets. No verb pressure.

**Verdict: zero new verbs required. The 11-verb constraint holds across all five steps.**

#### SRP — `enrich_metrics` decomposition without verb split

SRP at the verb level: "compute per-item quality metrics" is one responsibility. SRP at the implementation level: three distinct concerns. The resolution is module decomposition (§3.3), not verb splitting. Satisfies SRP at both levels while keeping the verb surface at 11.

#### OCP — schema extension point

The `:EntryPoint { kind: mcp|cli|http|cron }` string discriminator is the OCP extension point. Adding gRPC = addition (new detector routine), not modification. The VSB Cypher query BFS-walks all `:EntryPoint` nodes regardless of `kind`. Open for extension, Closed for modification.

#### LSP — `StoreBackend` unchanged

RFC-035 indexes are internal to `cfdb-petgraph::index`. They do not appear in `StoreBackend`. VSB BFS and raid joins are Cypher compositions. `query_with_input()` HTTP wire-up is a CLI/HTTP layer concern. `cfdb-petgraph::PetgraphStore` satisfies both `StoreBackend` and `EnrichBackend` contracts before and after all five v2 steps. No trait strengthening required.

#### DIP — one risk, one prescription

Step 3 AST re-walking must NOT call into `cfdb-extractor`. Prescription: `ast_signals.rs` invokes `syn::parse_file` directly from within `cfdb-petgraph`. Dep direction `cfdb-petgraph → cfdb-extractor` is forbidden (§3.3).

#### Component-level metrics (fan-in / fan-out / I / A / D)

| Crate | Ca | Ce | I | A | D | Zone |
|---|---|---|---|---|---|---|
| cfdb-core | 7 | 0 | 0.00 | 0.05 | 0.95 | Pain (accepted) |
| cfdb-concepts | 3 | 0 | 0.00 | 0.00 | 1.00 | Pain (accepted) |
| cfdb-query | 2 | 1 | 0.33 | 0.00 | 0.67 | Pain-adjacent |
| cfdb-petgraph | 2 | 3 | 0.60 | 0.00 | 0.40 | OK |
| cfdb-cli | 0 | 7 | 1.00 | 0.00 | 0.00 | Main Sequence |
| cfdb-extractor | 2 | 2 | 0.50 | 0.00 | 0.50 | OK |
| cfdb-hir-extractor | 2 | 1 | 0.33 | 0.50 | 0.17 | Main Sequence |
| cfdb-hir-petgraph-adapter | 1 | 3 | 0.75 | 0.00 | 0.25 | OK |
| cfdb-recall | 0 | 2 | 1.00 | 0.00 | 0.00 | Main Sequence |

Zone of Pain for `cfdb-core` and `cfdb-concepts` is accepted as intentional — maximally stable zero-dep foundation crates, evolving only via RFC-gated additive changes. v2 additions do not worsen any metric (no new crates, no new inter-crate deps).

#### Prescriptions (resolved in this RFC)

- **P1 — G6 determinism invariant for `test_coverage`.** Resolved in §4.
- **P2 — plan.yaml schema outside cfdb.** Resolved in §3.5 and §6.

### Rust Systems lens

**Verdict: RATIFY** (with prescriptions P-RS1 and P-RS2, both resolved in §3.1 / §4 of this RFC)

#### Entry-point detection — syn patterns

`:EntryPoint` + `EXPOSES` emission is fully implemented in `cfdb-hir-extractor/src/entry_point_emitter.rs` (issues #86 / #124 / #125). PLAN-v2 step 1 reduces to `REGISTERS_PARAM` emission + scar corpus.

Concrete detection contracts for REGISTERS_PARAM producers:

- **MCP `#[tool]` fn:** attribute-textual detection already ships. For REGISTERS_PARAM, parse `ast::Fn::param_list()` syntactically — names and type paths available without HIR. Each non-self parameter generates one REGISTERS_PARAM edge to a `:Param` node.
- **Clap `#[derive(Parser)]` struct:** walk struct fields for `#[arg(...)]` attributes. Field name = param name; field type path = param type. Each `#[arg(...)]`-annotated field generates one REGISTERS_PARAM edge.
- **Axum `Router::route(...)` handler:** handler params are not extractable without calling into the handler fn's signature. REGISTERS_PARAM for http_route kind is deferred to v3. For v2, http_route `:EntryPoint` nodes emit empty params list — documented as not yet supported.

The `hir` feature flag must be preserved. `cfdb-hir-extractor` cannot be a non-feature-gated mandatory dep due to 90–150s ra-ap-* cold compile cost.

#### Quality enrichment performance at 148k-node scale

`enrich_metrics` for `unwrap_count` + `cyclomatic`: AST walk using `syn::parse_file` is rayon-parallelizable with sort-before-emit for G1. Estimated 2–5s for 15k `:Item{Fn}` nodes. The `syn` dep must be added to `cfdb-petgraph/Cargo.toml` behind the `quality-metrics` feature flag. `cfdb-petgraph` must NOT depend on `cfdb-extractor` for the AST walk helpers — invoke `syn::parse_file` directly (SOLID DIP endorsed).

`dup_cluster_id`: O(n log n) sort + O(n) scan. Sequential. Cluster id = sha256 of sorted member qnames (DDD prescription). Deterministic under any iteration order.

`test_coverage`: excluded from G1; subject to G6 (toolchain-version-stable only).

#### Feature-flag topology

```toml
# cfdb-petgraph/Cargo.toml
[features]
quality-metrics = ["dep:syn"]     # enables enrich_metrics real implementation
llvm-cov = ["quality-metrics"]    # enables test_coverage subfeature
```

The `hir` feature flag in cfdb-cli remains unchanged.

#### Trait object safety and orphan rule

`StoreBackend` and `EnrichBackend` are object-safe and used as `&dyn StoreBackend` in `cfdb-cli`. v2 adds no new methods to these traits; object safety is preserved. `HirDatabase` remains non-object-safe and correctly isolated. No orphan-rule violations in current or proposed v2 code.

#### Dependency DAG — no new edges

v2 adds zero new crate dependency edges. The `syn` dep in `cfdb-petgraph` under the `quality-metrics` feature flag is the only new `Cargo.toml` entry, and it runs stable-to-stable. DAG remains acyclic.

#### Peer challenge resolved

P-RS1 (`:Param` node id collision in step 1) is resolved in §3.1 CP1 — REGISTERS_PARAM targets the existing `:Param` nodes from `HAS_PARAM`, same id formula, no divergence.

## 6. Non-goals

- **LLM enrichment.** Deferred until a consumer use case requires it.
- **Multi-language extraction.** Rust-only for v2.
- **IDE integration.** No LSP server, no VS Code extension.
- **Auto-fix / code modification.** cfdb is read-only at the source level.
- **Plan.yaml parser inside cfdb.** Consumer workspace owns the YAML parser. cfdb ships query templates only (§3.5 CP7).
- **`/gate-raid-plan` skill implementation.** Skill lives in the consumer workspace; v2 delivers the queries and the bucket-set convention, not the skill.
- **HTTP route handler param registration.** Deferred to v3 (requires HIR fn-signature resolution of extracted handler fns).
- **Incremental enrichment.** v2 ships stateless full re-walk. Incremental enrichment is a future RFC if perf forces it.
- **UI / dashboard for HSB clusters.** Consumer concern.

## 7. Issue decomposition

Each step is one vertical-slice issue. Every `Tests:` block follows CLAUDE.md §2.5 (four rows). Issues are filed after ratification; this section is the architects' prescription.

### Issue 036-1 — REGISTERS_PARAM emission + :Param producer coverage (step 1)

**Scope:** extend `cfdb-hir-extractor::entry_point_emitter` to emit `REGISTERS_PARAM` edges for MCP `#[tool]` fn params and clap `#[arg(...)]` struct fields. Reuse existing `:Param` nodes via `cfdb-core::qname` id formula. Ship scar corpus fixtures. Bump `SchemaVersion` to v0.3.0 (additive: new edge producer; existing label).

**Tests:**
- **Unit:** For each detector (MCP / clap), parse a minimal `syn` fixture and assert the emitted `REGISTERS_PARAM` edges reference `:Param` node ids matching the handler's `HAS_PARAM` output. No I/O.
- **Self dogfood (cfdb on cfdb):** extract cfdb's own workspace; assert every `:EntryPoint{kind:cli_command}` has `REGISTERS_PARAM` edges equal in count to the clap `#[arg(...)]` fields on its handler struct. Zero missing, zero extra.
- **Cross dogfood (cfdb on graph-specs-rust at pinned SHA):** extract graph-specs-rust; assert the companion produces zero rule-rows under all existing `.cfdb/queries/` ban rules after the schema bump. Exit 30 on any rule row blocks merge.
- **Target dogfood (on downstream target workspace at pinned SHA):** extract; report `:EntryPoint` count by kind + total REGISTERS_PARAM edge count in the PR body for reviewer sanity-check. Expected: non-zero CLI + MCP entry-point counts.

### Issue 036-2 — VSB detector query + scar corpus (step 2)

**Scope:** ship `.cfdb/queries/vsb-multi-resolver.cypher` + three scar fixtures under `crates/cfdb-extractor/tests/fixtures/vsb/` (Param-Effect Canary, MCP Boundary Fix AC, compound stop). No schema delta.

**Tests:**
- **Unit:** Query runs against each scar fixture; returns exactly one candidate per intentionally-scarred entry point; returns zero for the clean baseline.
- **Self dogfood:** run query on cfdb's own keyspace; assert zero candidates (cfdb's own entry points should have exactly one resolver per param by construction). Any finding is either a real bug to fix or a carve-out to document in `KNOWN_GAPS.md`.
- **Cross dogfood:** run on graph-specs-rust; zero candidates expected.
- **Target dogfood:** run on downstream target workspace; report candidate count in PR body; investigate any finding as part of the target's separate issue tracker.

### Issue 036-3 — enrich_metrics real implementation behind quality-metrics feature (step 3)

**Scope:** implement `PetgraphStore::enrich_metrics` in `cfdb-petgraph/src/enrich/metrics/{mod.rs, ast_signals.rs, coverage.rs, clustering.rs}`. Add `syn` as optional dep. Declare `quality-metrics` and `llvm-cov` features. Populate four attributes with `Provenance::EnrichMetrics`. Bump `SchemaVersion` to v0.3.1 (additive attributes). Document G6 invariant in `SchemaDescribe`.

**Tests:**
- **Unit:** `ast_signals.rs` — for each minimal syn fixture (zero unwraps / three unwraps / zero branches / high-complexity fn), assert the computed counts. `coverage.rs` — for a canned cargo-llvm-cov JSON blob, assert `test_coverage` mapping. `clustering.rs` — for two items with matching `signature_hash`, assert both receive the same `dup_cluster_id` = sha256 formula output.
- **Self dogfood:** run `enrich_metrics` on cfdb; assert determinism by running twice and sha256-diffing the canonical dump (excluding `test_coverage` per G6). Assert every `:Item{kind:Fn}` has non-None `unwrap_count` and `cyclomatic`.
- **Cross dogfood:** run on graph-specs-rust with `quality-metrics` feature; assert zero rule-rows on all ban rules; assert identical canonical-dump-minus-coverage sha256 across two consecutive runs.
- **Target dogfood:** run on downstream target workspace with feature; report mean `cyclomatic`, P95 `unwrap_count`, and HSB cluster count in PR body.

### Issue 036-4 — HSB multi-signal cluster query (step 4)

**Scope:** ship `.cfdb/queries/hsb-cluster.cypher` using step-3 attributes and RFC-035 indexes. Document the unit-struct / empty-type carve-out.

**Tests:**
- **Unit:** on a synthetic 3-crate fixture with two known duplicates (one same-name, one synonym-renamed), assert the cluster query returns both candidates with `dup_cluster_id` populated.
- **Self dogfood:** run on cfdb; investigate every cluster; zero unacknowledged clusters (each must be either fixed or documented in `KNOWN_GAPS.md` before merge).
- **Cross dogfood:** run on graph-specs-rust; zero clusters expected (companion is one crate).
- **Target dogfood:** run on downstream target workspace; report cluster count grouped by signal-match pattern; reviewer sanity-checks top-10 clusters are real duplicates.

### Issue 036-5 — Raid plan validation queries + plan.yaml bucket convention (step 5)

**Scope:** ship five Cypher templates under `examples/queries/raid/` + documentation of the bucket convention (portage / rewrite / glue / drop + `source_context` scalar) in `docs/raid-plan-schema.md`. No schema delta. No parser inside cfdb.

**Tests:**
- **Unit:** synthetic workspace fixture (two crates, one raid plan with deliberately-dangling-drop). Assert each of the five queries returns the expected candidate set (completeness flags exactly the omitted item; dangling-drop flags the referencing item; etc.).
- **Self dogfood:** author a deliberate raid plan for `cfdb-extractor` (fictional raid, not to be executed); run all five queries; assert the pass/fail pattern matches hand-computed expectation.
- **Cross dogfood:** run the five queries against graph-specs-rust with an empty plan; all return empty; assert zero false-positive holes.
- **Target dogfood:** (DEFERRED to the first real raid in the consumer workspace — this issue does not run queries on an external target.)

### Issue 036-spec-hygiene — specs amendments (land alongside 036-1)

**Scope:** amend `specs/concepts/cfdb-core.md`, `cfdb-petgraph.md`, `cfdb-hir-extractor.md` per `council/036/verdicts/*-specs.md`. Correct pre-existing spec drift (Provenance 2→6 variants; EnrichBackend 4→7 defaults; EmitStats 3→5 fields). Add new entries: `:EntryPoint`, `EXPOSES`, `REGISTERS_PARAM`, `:Param` graph-node label. Rename query-AST `Param` → `ParamBinding`. Declare G6 invariant inline. Document OCP extension policy on `Label`. Document Zone of Pain intentionality.

**Tests:**
- **Unit:** `graph-specs check --specs specs/concepts/ --code crates/` returns zero violations after the amendments land.
- **Self dogfood:** `make graph-specs-check` passes in cfdb's CI.
- **Cross dogfood:** graph-specs-rust's cross-dogfood step returns exit 0 against the new cfdb schema.
- **Target dogfood:** (N/A — spec hygiene is internal to cfdb.)

---

**End of RFC-036 draft.** R1 council complete; all nine open items collapse to seven convergence points, all resolved above. Pending R2 re-ratification from all four lenses.

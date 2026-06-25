# RFC-047a — Impact query mechanics (complement to RFC-047)

- **Status:** **RATIFIED** — architect council 2026-06-25 (agent-team, 4 lenses). ddd-specialist + clean-arch **RATIFY**; rust-systems + solid-architect **REQUEST CHANGES** → amendments applied (§5, §7) → **RATIFY ×4**. Verdicts: [`council/RFC-047a-complement/`](../council/RFC-047a-complement/) (BRIEF + 4 lens files). Q1 (open-form depth) converged **unbounded-via-visited-set**; Q2 (explicit-bound clamp) converged **unconditional fix in 47-0**.
- **Parent:** [RFC-047 — Impact / blast-radius query](RFC-047-impact-blast-radius.md), **RATIFIED**, merged develop `018e766` (#484).
- **Why a complement, not an edit:** RFC-047 is ratified and merged. This document records (a) a **correction of record** — a foundational council claim was false — and (b) the **three mechanical blockers** RFC-047 §3.2 silently assumed away. It re-cuts slices 47-0 / 47-A accordingly. The parent stays as the record; this supersedes its §3.2 mechanics.
- **Schema impact:** **none.** No node/edge label, no attribute, no `SchemaVersion` bump. (One change touches the Cypher-subset grammar — a query-language addition, not a keyspace schema change. Called out in §4.)
- **Companion:** none (no keyspace schema surface → no `graph-specs-rust` lockstep).
- **Discovered by:** session 2026-06-25, first implementation touch of slice 47-0 (#488). The list-binding the slice was filed to "land" already shipped; running the canonical query surfaced what actually blocks `impact`.

---

## 1. Problem — RFC-047 rests on a false premise and omits three real blockers

RFC-047 §3.2 and §5 (clean-arch) assert, as the single blocking finding that gated 47-0:

> **"Council finding (clean-arch, lead-verified): no list-binding path exists today."**

**This is false, and was false at deliberation time.** The in-process path RFC-047 §3.2 prescribes for `impact` — `parse(template)` → `query.params.insert(name, Param::List(..))` → `store.execute` — already ships and runs in production:

- `cfdb_core::Param::List(Vec<PropValue>)` exists (`crates/cfdb-core/src/query/ast.rs:54-57`).
- The evaluator resolves a list param for `IN`: `eval_expr_list` returns `Some(items.clone())` for `Param::List` (`crates/cfdb-petgraph/src/eval/predicate.rs:115-117`); `Predicate::In` consumes it (`predicate.rs:26-33`).
- The CLI already binds `Param::List` end-to-end: `param_resolver.rs` `list:<a,b,c>` and `context:<name>` forms (`crates/cfdb-cli/src/param_resolver.rs:8,90` — #145), consumed by the shipped `check-predicate` verb (#147), exercised over real fixtures by `raid_plan_queries.rs` (#205, which binds `Param::List` into `IN $portage`/`$drop` templates).
- The parser accepts `WHERE x IN $listparam` (`parser_subquery.rs`, `parser_patterns.rs`).

The council inspected only the **raw `query --params` / `--input` surface** (`commands/query.rs:39` `--input` stub, `:104` `bind_single_param` rejects JSON arrays) — which RFC-047 §3.2 itself scopes **out** ("*not* by routing through the CLI `--params`/`--input` surface"). So the cited evidence never bore on the path the slice actually uses. (This is the failure mode flagged in memory *council foundation claims need verification*.)

**Consequence:** slice 47-0 as filed ("land list-valued param binding") is moot — that capability is done. But running the canonical query (RFC-047 §3.2) surfaces **three blockers RFC-047 never names**, all of which actually stand between cfdb and a working `impact`:

| # | Blocker | Evidence | Effect on RFC-047 §3.2 query |
|---|---------|----------|------------------------------|
| B1 | **Open-range `*N..` does not parse** | `crates/cfdb-query/src/parser/match_clause.rs:82-86` — `range` requires `digits()` on **both** sides of `..` | `(seed)<-[:CALLS*1..]-(affected)` fails to parse at the relationship |
| B2 | **Var-length BFS silently caps at depth 5** | `DEFAULT_VAR_LENGTH_MAX = 5` (`crates/cfdb-petgraph/src/eval/mod.rs:64`); applied to **all** var-length in `traverse_bfs` (`eval/pattern/path.rs:205-208`) | Reverse reachability truncated at 5 hops — the truncation RFC-047 §3.2 explicitly rejected |
| B3 | **`extract_workspace` emits no resolved `CALLS`** | `crates/cfdb-extractor/src/lib.rs:18` — "Out of scope for v0.1: resolved cross-crate `CALLS` (Item → Item)"; it emits `INVOKES_AT` only | The prescribed self-dogfood cannot use the lightweight library extract path |

These were verified by writing the RFC-047 §3.2 query and running it (proofs: `.proofs/488-impact-seed-binding.txt` — parse failure on `*1..`; `.proofs/488-fixture-anchor.txt` — green once bounded to `*1..5`).

## 2. Scope

**Ships (this complement + its re-cut slices):**
1. **Correction of record** (§1) — RFC-047 §3.2/§5's "no list-binding path" finding is retracted; the binding ships.
2. **B1 fix** — extend the Cypher-subset grammar so an open upper bound `*N..` parses to `(N, u32::MAX)`.
3. **B2 fix (unconditional — lands in 47-0, before 47-A)** — align the evaluator to `DEFAULT_VAR_LENGTH_MAX`'s own documented contract: **explicit finite bounds are honoured as written** (the `.min(DEFAULT_VAR_LENGTH_MAX)` clamp is dropped for them), and the open-form `*N..` is **unbounded-via-visited-set** (Q1, council-converged). This un-truncates **shipped** queries that are silently clamped today — including the live `.cfdb/queries/vsb-multi-resolver.cypher:67` ban rule (`*1..10` → 5) run by the CI `violations` gate.
4. **B3 resolution** — 47-A's self-dogfood is re-specified to run against a **HIR-extracted (CALLS-resolved) keyspace**, not `extract_workspace`.
5. **Re-cut slices** 47-0 / 47-A (§7).

**Does not ship:** any new fact, node/edge label, attribute, `SchemaVersion` bump, or trait verb (the §3.1/§4 invariants of RFC-047 are untouched — `impact` is still pure composition).

## 3. Design

### 3.1 B1 — open var-length range parses (Cypher-subset grammar)

Today (`match_clause.rs:82-86`):

```rust
let range = just('*')
    .ignore_then(digits())          // lower bound — required
    .then_ignore(just("..").padded())
    .then(digits())                 // upper bound — REQUIRED ⇒ `*1..` fails
    .boxed();
```

Change: make the upper `digits()` optional; absent upper ⇒ `u32::MAX` sentinel. The AST tuple `var_length: Option<(u32, u32)>` (`ast.rs:108`) is **reused unchanged** — `*1..` becomes `Some((1, u32::MAX))`. No new AST variant (YAGNI). Grammar surface added: `*N..` (open upper). `*N..M` (closed) and `[:LABEL]` (no quantifier) are unchanged.

This is a **query-language** addition (Cypher subset), not a keyspace schema change — no `SchemaVersion` bump, no companion lockstep. It is still RFC-gated as a "Cypher subset construct" (`CLAUDE.md §3`), which this complement satisfies.

### 3.2 B2 — honour explicit bounds; default-cap only the open form

`DEFAULT_VAR_LENGTH_MAX`'s own doc states its intended contract (`eval/mod.rs:62`):

> `/// Maximum BFS depth when a variable-length pattern OMITS its upper bound.`

But the code contradicts the doc twice:
- The **parser** never produced an omitted-upper form (B1), so the documented case was **unreachable**.
- The **evaluator** applies the cap to *every* var-length, clamping explicit `*1..10` → 5 (`path.rs:208`: `max_depth.min(DEFAULT_VAR_LENGTH_MAX.max(min_depth))`).

So the current behaviour is a **latent inconsistency**, independent of `impact`. It is **not a regression** — git history shows `DEFAULT_VAR_LENGTH_MAX` has been `5` since the initial portage (`8ed8b97`); it was never `8` in cfdb (the `= 8` cited in `examples/queries/vertical-split-brain.cypher:73` is a stale comment from a higher pre-portage value). The effect is that **shipped queries authored expecting deeper traversal have always been silently clamped to 5**:
- `.cfdb/queries/vsb-multi-resolver.cypher:67` — `CALLS*1..10` → 5 (this is a **live CI ban rule**: `cfdb violations` misses split-brain call chains at depth 6–10 today).
- `examples/queries/vertical-split-brain.cypher:119-120`, `…-drop.cypher:135-136` — `CALLS*1..8` → 5.

**There is no performance justification** for the clamp on explicit bounds — `traverse_bfs` dedupes by a visited-set whose `insert` guard is at enqueue time (`path.rs:230`), *not* gated behind `max_depth` (`path.rs:216-232`); each node enqueues at most once, so the walk is O(V+E) regardless of `max_depth` — the depth limit only prunes the frontier earlier. (RFC-047 §3.2 rust-systems note already states the BFS is "O(V+E), sub-second.")

Fix (aligns code to its own contract) — **council Q1: RESOLVED unbounded-via-visited-set**:
- **Explicit finite upper bound** (`*N..M`, `M < u32::MAX`): honour `M` as written — drop the `DEFAULT_VAR_LENGTH_MAX` clamp for this case.
- **Open upper bound** (`*N..` ⇒ `M == u32::MAX`): treat as truly unbounded — the visited-set is the only bound (matches RFC-047 §3.2's "unbounded by default, O(V+E)" intent and the *already-unbounded* enrich-side `bfs_call_graph` at `enrich/reachability.rs:246`). `DEFAULT_VAR_LENGTH_MAX` is retained only as the fallback for this open branch and **must carry a code comment at the `u32::MAX` branch in `traverse_bfs`** naming that it applies to the open form only, not explicit bounds (else the constant is a readability trap — rust-systems + solid).

There are **zero open-form queries in the tree today** (the parser rejected them), so the open-form policy affects only new queries — its blast radius is bounded to `impact` and future authors, not existing rules.

### 3.3 B3 — the impact dogfood runs against a HIR-resolved keyspace

Resolved `Item→Item CALLS` edges are produced only by the **HIR extraction path** (`cfdb-hir-extractor`, `--hir` feature) — `crates/cfdb-hir-extractor/src/emit.rs` / `call_site_emitter/`. The syn-based `extract_workspace` (`cfdb-extractor`) deliberately stops at `INVOKES_AT` call sites + `synthesize_referenced_items` stub nodes (`lib.rs:18,254`); it never resolves the cross-crate call graph. RFC-047 §1's "2197-function `CALLS` graph" exists **because** cfdb self-extracts with HIR.

Therefore the 47-A self-dogfood cannot reuse the `predicate_library_dogfood.rs` pattern (which calls `extract_workspace` and asserts on non-CALLS facts). It must obtain a CALLS-populated keyspace via the real pipeline — `cfdb extract --hir` to a temp keyspace, then load + query. This is heavier (the `hir` feature pulls `ra_ap_*`); 47-A's `Tests:` block is re-specified to name it (§7).

### 3.4 What is salvage

The regression test written at first touch (`crates/cfdb-cli/tests/impact_seed_binding.rs`) is kept: it pins the **list-binding + reverse var-length traversal + `IN $seeds` membership** composition on a fact-injected fixture (green today at `*1..5`; switches to the open `*1..` form once B1 lands). It moves into the re-cut 47-0.

## 4. Invariants

- **Determinism unchanged.** BFS over a fixed `CALLS` set is deterministic; honouring explicit bounds and a fixed open-form policy keeps re-extract byte-stability (the depth rule is a pure function of the query). G6/sha256 dump path untouched.
- **No keyspace schema surface.** No label/attribute/`SchemaVersion` change → no `graph-specs-rust` lockstep. The grammar change (§3.1) is a query-language construct, not a stored-fact change.
- **Recall unaffected.** No new fact kind; `cfdb-recall` corpus is not extended (the recall gate concerns extracted facts; this adds none).
- **No metric ratchet.** Nothing here adds a baseline/ceiling/allowlist (`CLAUDE.md §6 rule 8`).
- **Verb ceiling untouched.** `impact` remains pure composition of `query`; 7+7 trait surface unchanged.

## 5. Architect lenses — contested questions

Council 2026-06-25 (agent-team, mailbox + shared task list). Verdict files: `council/RFC-047a-complement/<lens>.md`.

| Lens | Verdict | One-line |
|---|---|---|
| ddd-specialist | **RATIFY** | No new label/attribute/`SchemaVersion`, no homonym; query-level `traverse_bfs` distinct from enrich-level `bfs_call_graph`. |
| clean-arch | **RATIFY** | B1→`cfdb-query`, B2→`cfdb-petgraph` `traverse_bfs`, B3 dogfood at composition root; dependency direction intact; HIR quarantine preserved. Confirmed the 3 live truncations. |
| rust-systems | **REQUEST CHANGES → RATIFY** | O(V+E) verified (`path.rs:216-232`); open form unbounded. Amendments: policy comment + 47-A Tests prescription. Found the live `vsb-multi-resolver` truncation. |
| solid-architect | **REQUEST CHANGES → RATIFY** | 47-0/47-A SRP-clean; `(u32,u32)` reused (no new variant). Amendments: named parser unit test + split `impact_hir_dogfood.rs`. |

All amendments are test-prescription / documentation (the design is sound) and are applied in §3.2 (policy comment) and §7 (Tests blocks) — flipping both REQUEST-CHANGES lenses to RATIFY. Per-lens pre-analysis, all confirmed at `file:line`:

- **clean-arch** — B1 in `cfdb-query` (depends only on `cfdb-core`), B2 in `cfdb-petgraph` `traverse_bfs` (no `StoreBackend` change), B3 dogfood in `cfdb-cli/tests` (composition root). `impact` stays zero-logic composition. RATIFY.
- **rust-systems (Q1/Q2 lead)** — visited-set O(V+E) confirmed (`path.rs:230` insert is at enqueue, not gated by `max_depth`); open form unbounded; explicit-bound clamp is a live bug. RATIFY on the 2 amendments.
- **solid-architect** — B1+B2 cohere as 47-0 (one reason to change: var-length bounding); the HIR dogfood is CCP-split into its own file/run-schedule. RATIFY on the 3 amendments.
- **ddd-specialist** — grammar + eval + dogfood-wiring only; "blast radius" appears once as test-comment prose, not a type/label. RATIFY.

**Cross-challenge questions (Phase B, mailbox) — outcomes:**

- **Q1 (crux) — open-form `*N..` semantics.** → **RESOLVED: visited-set-unbounded** (rust-systems ⇄ clean-arch ⇄ solid converged, no dissent). O(V+E) confirmed at `path.rs:216-232` (`visited.insert` at `:230` is at enqueue, not gated by `max_depth`), so a cap buys nothing; unbounded matches RFC-047 §3.2 and the already-unbounded enrich-side BFS (`reachability.rs:246`). Mandatory policy comment at the `u32::MAX` branch in `traverse_bfs` (folded into §3.2).
- **Q2 — explicit-bound clamp as a latent bug.** → **RESOLVED: unconditional fix in 47-0** (rust-systems ⇄ clean-arch converged). Three shipped queries truncated today (live `.cfdb/queries/vsb-multi-resolver.cypher` + two `examples/queries`); no Q1 dependency; lands in 47-0 with a named regression test. **Author caveat:** un-truncating may surface *new* split-brain rows on cfdb-self → 47-0 re-verifies the `violations` gate stays zero, boy-scouting any newly-exposed finding (`CLAUDE.md §7`).
- **Q3 — 47-0 / 47-A boundary + HIR-dogfood.** → **RESOLVED** (solid ⇄ rust-systems ⇄ ddd converged). Mechanics (B1+B2) in 47-0; canonical query + dogfood in 47-A. The HIR dogfood is **CCP-split** into its own `impact_hir_dogfood.rs`, gated `#[cfg_attr(not(feature = "integration-live"), ignore)]`, calling `cfdb_hir_extractor::build_hir_database` then `extract_call_sites` **in-process** (no `extract_workspace_hir` surface, `lib.rs:89-92`; no shell-out). `cfdb-hir-extractor` becomes a `cfdb-cli` `[dev-dependencies]` behind `integration-live`.
- **Q4 — correction of record.** → **RESOLVED: complement supersedes** (all four; amending a ratified RFC in place corrupts the audit trail — ddd). A forward-pointer is added to RFC-047 §3.2/§5 → RFC-047a (clean-arch).

## 6. Non-goals

- A `--max-depth` **CLI flag** — deferred to 47-B (it maps a CLI arg to the now-expressible `*1..N`); not built here. (RFC-047 §3.2 mentioned it; this complement only makes the *query form* it maps to expressible.)
- Forward "what does X depend on" / signature-precise seeding — unchanged from RFC-047 §6.
- Raising `DEFAULT_VAR_LENGTH_MAX` as a global number for *all* queries — out of scope; B2 changes *when* it applies, not its value.
- Any new fact, edge weight, or rendering — unchanged from RFC-047 §6.

## 7. Re-cut issue decomposition

Supersedes RFC-047 §7 for 47-0 / 47-A. 47-B / 47-C are unchanged.

### 47-0 (re-framed) — Var-length reverse-reachability query mechanics
The original 47-0 ("land list-binding") is **closed: capability pre-exists** (§1). The re-framed 47-0 lands **B1 + B2 unconditionally** so the canonical query is expressible and shipped queries stop being silently truncated, and pins the composition with the salvage test. **Implementation notes:** (i) `traverse_bfs` carries a comment at the `u32::MAX` (open-form) branch naming that `DEFAULT_VAR_LENGTH_MAX` applies to the open form only, not explicit bounds; (ii) B2 un-truncates the live `.cfdb/queries/vsb-multi-resolver.cypher` (`*1..10`) and the `examples/queries/*.cypher` (`*1..8`) rules — **re-run `cfdb violations` on cfdb-self and confirm the gate stays zero**, boy-scouting any newly-surfaced split-brain finding (`CLAUDE.md §7`).
```
Tests:
  - Unit: (a) a DEDICATED parser test (own fn) — `parse("MATCH (a)<-[:CALLS*1..]-(b) RETURN a")` is Ok and `edge.var_length == Some((1, u32::MAX))`; (b) an evaluator test — `*1..10` over a ≥6-hop fixture traverses all 10 hops (no clamp), `*1..3` honours 3; (c) the open form follows the ratified visited-set-unbounded policy. Plus the fixture composition test (reverse `<-[:CALLS*1..]-` + `IN $seeds` Param::List ⇒ caller union; single-seed control proves membership filtering) — `crates/cfdb-cli/tests/impact_seed_binding.rs`, with `IMPACT_QUERY` updated from `*1..5` to the open `*1..` in this PR.
  - Self dogfood (cfdb on cfdb): the B2 fix re-runs the `violations` gate on cfdb-self and asserts it stays zero (or fixes/files any newly-exposed row). (The CALLS-graph IMPACT dogfood is 47-A's — needs HIR, §3.3.)
  - Cross dogfood (graph-specs-rust at pinned SHA): none — rationale: query-language change, no keyspace schema / ban surface.
  - Target dogfood (qbot-core at pinned SHA): none — rationale: pure mechanics; end-to-end signal reported by 47-B.
```

### 47-A (amended) — Canonical reverse-reachability query + HIR dogfood
Add the parameterised query (RFC-047 §3.2, now parseable via 47-0) and assert it against a **HIR-extracted** cfdb-self keyspace.
```
Tests:
  - Unit: query-builder produces the expected Query AST for a given seed list.
  - Self dogfood (cfdb on cfdb): a SEPARATE `crates/cfdb-cli/tests/impact_hir_dogfood.rs`, gated `#[cfg_attr(not(feature = "integration-live"), ignore)]` (CI 5-min budget — `ra_ap_*` cold compile is 90–150 s), calling `cfdb_hir_extractor::build_hir_database(&root)` then `extract_call_sites(&db, &vfs)` **in-process** (no shell-out; no `extract_workspace_hir` surface, `lib.rs:89-92`) to build a resolved-CALLS keyspace; seed a known leaf fn in cfdb-core, assert its known callers in cfdb-petgraph/cfdb-cli appear. `cfdb-hir-extractor` added to `cfdb-cli` `[dev-dependencies]` behind `integration-live`. (NOT `extract_workspace` — §3.3.)
  - Cross dogfood: none — rationale: no schema/ban surface.
  - Target dogfood (qbot-core): report blast-radius size for one representative changed fn in PR body.
```

### 47-B / 47-C — unchanged
Per RFC-047 §7. 47-B additionally owns the `--max-depth` CLI flag (§6), mapping it to `*1..N` over the mechanics 47-0 lands.

# RFC-056 — `GraphPort`: split `cfdb-petgraph`'s enrichment passes behind a port (strangler-fig)

- Status: **DRAFT**
- Refs: cfdb self-audit 2026-08-17 (session `220c95c4`, artifact `cfdb Self-Audit`) — major finding: `cfdb-petgraph` is the repo's big-ball-of-mud. Characterization safety net: PR #575 (commit `c21f89b` on develop) pins the current `EnrichBackend` dispatch surface byte-for-byte. Precedent: RFC-031 (original `EnrichBackend` / `StoreBackend` port split).

## 1. Problem

`cfdb-petgraph` bundles three responsibilities that change for unrelated reasons and have **zero shared commit history** between two of them (git-log evidence, self-audit 2026-08-17): the `StoreBackend` adapter (petgraph storage), the Cypher evaluator (`eval/`), and 7 enrichment passes (`enrich/`, dispatched via `EnrichBackend`). CRP/CCP: three axes of change, one crate, one `Cargo.toml`, one compile unit.

The concrete coupling (verified at source, not asserted from memory):

- `KeyspaceState` (`crates/cfdb-petgraph/src/graph.rs:27`) is `pub(crate)` and holds `pub(crate) graph: StableDiGraph<Node, Edge>` (`graph.rs:30`) directly.
- All 7 enrichment `run()` functions take `&mut KeyspaceState` and reach straight into petgraph internals: `enrich/reachability.rs` imports `petgraph::stable_graph::NodeIndex`, `petgraph::visit::EdgeRef`, `petgraph::Direction` and walks `state.graph.edges_directed(idx, Direction::Outgoing)` directly (`reachability.rs:65-67,212,252-254`). `bounded_context.rs`, `concepts.rs`, `attr_call_resolution.rs` do the same for `NodeIndex`-keyed reads/writes (`node_weight_mut(idx).props.insert(...)`, `bounded_context.rs:163-164`).
- No port exists between enrichment *logic* and the petgraph *storage engine*. `eval/` has the identical shape (`eval/mod.rs:113`: `pub(crate) state: &'a KeyspaceState`) and is explicitly **out of scope** here (§6).

This is not "one big file" — it is an absent seam. A pass cannot be tested, changed, or reasoned about without the concrete petgraph representation in scope, and nothing in the type system stops a new pass from reaching further into `StableDiGraph` internals tomorrow.

**Why now, why this shape:** the prior session built a full 7-verb × 4-branch characterization matrix and shipped the 6 missing pinning tests + 1 CI step (PR #575) specifically so this extraction can be checked byte-for-byte against a known-good baseline. This RFC is the extraction plan; it does **not** re-judge whether any pass's current behavior is correct — that is an explicit, separate, later question (Michael Feathers discipline: characterize, then strangle, then re-evaluate).

## 2. Scope

Ships:

- **`GraphPort` / `GraphPortStore`** — two new traits in `cfdb-core`, siblings of `StoreBackend`/`EnrichBackend` (same precedent as RFC-031's split). `GraphPort` is the per-keyspace read/write surface an enrichment pass needs; `GraphPortStore` is the per-store factory that resolves a keyspace into a `GraphPort` (mirrors the `require_keyspace` guard `enrich_backend.rs:26-31` already does once per verb call today).
- **`cfdb-enrich`** — new workspace crate. Depends **only** on `cfdb-core` (+ `cfdb-concepts`, + feature-gated `git2`/`syn`/`sha2` — same optional deps `cfdb-petgraph` carries today for the same passes). Holds all 7 pass modules, rewritten against `GraphPort` — zero `petgraph` import possible, because `cfdb-enrich` never adds the `petgraph` crate as a dependency (compiler-enforced, not convention).
- **`EnrichEngine<S: GraphPortStore>`** in `cfdb-enrich`, implementing `cfdb_core::enrich::EnrichBackend` generically over any `GraphPortStore` implementor.
- `cfdb-petgraph` implements `GraphPortStore` (`KeyspaceState` implements `GraphPort` directly — thin delegation to its existing `pub(crate)` methods, now reached through a `pub` trait instead of raw field access). `cfdb-petgraph` **drops** its `EnrichBackend` impl, `enrich_backend.rs`, and `enrich/` entirely once all 7 slices land.
- Composition-root cutover in `cfdb-cli`: `store.enrich_x(...)` → `EnrichEngine::new(&mut store).enrich_x(...)`.
- `.cfdb/workspace-dep-rules.toml` gains a `[cfdb-enrich]` section; `cfdb-petgraph`'s `forbidden` list gains nothing new (still never depends on a sibling adapter) but its `allowed` list drops `syn`/`sha2`/`git2` once the last feature-gated pass (metrics, git_history) moves.
- `.gitea/workflows/ci.yml` gains a slim/full test matrix entry for `cfdb-enrich` mirroring the one just added for `cfdb-petgraph` (git-enrich / quality-metrics off-by-default).

Does **not** ship (see §6): any change to `eval/`; any change to Node/Edge/`:Concept`/`:RfcDoc` wire shape; any `SchemaVersion` bump; any behavior change to any of the 7 passes — the acceptance bar for every slice is "characterization tests + self-dogfood diff are byte-identical to the pre-slice baseline."

## 3. Design

### 3.1 The port

```rust
// cfdb-core::enrich (or a new cfdb-core::graph_port sibling module)

pub enum Direction { /* reuse cfdb_core::query::ast::Direction — see §3.3 */ }

pub trait GraphPort {
    fn node_by_id(&self, id: &str) -> Option<&Node>;
    fn nodes_with_label(&self, label: &Label) -> Vec<String>;      // ids, sorted — see §4 determinism
    fn neighbors(&self, id: &str, dir: Direction) -> Vec<(EdgeLabel, String)>; // (edge label, other-endpoint id)
    fn set_attr(&mut self, id: &str, key: &str, value: PropValue) -> bool;    // false = unknown id
    fn ingest_nodes(&mut self, nodes: Vec<Node>);
    fn ingest_edges(&mut self, edges: Vec<Edge>);
}

pub trait GraphPortStore {
    fn graph_port(&mut self, keyspace: &Keyspace) -> Result<&mut dyn GraphPort, StoreError>;
    fn workspace_root(&self) -> Option<&Path>;
}
```

`GraphPort` is dyn-safe on purpose — no generics, no GATs. A "per-keyspace typed view" design (associated-type keyspace handle) was considered and rejected: it buys nothing here (every pass resolves one keyspace once, per today's `enrich_backend.rs` guard pattern) and would be the first GAT in the workspace for no consumer benefit (rust-systems trim, pre-council).

`node_by_id` / `nodes_with_label` / `neighbors` map onto `KeyspaceState`'s existing `pub(crate)` methods almost verbatim: `nodes_with_label` already exists (`graph.rs:301-306`, returns sorted `NodeIndex`es — the port version resolves each to its `id` before returning, same order); `neighbors` replaces the direct `state.graph.edges_directed(idx, dir)` calls in `reachability.rs`/`attr_call_resolution.rs`/`bounded_context.rs`; `set_attr` replaces the direct `node_weight_mut(idx).props.insert(...)` pattern (`bounded_context.rs:163-164`); `ingest_nodes`/`ingest_edges` are literal 1:1 forwards to `KeyspaceState::ingest_nodes`/`ingest_edges` (`graph.rs:122,264` — already exist, already take `Vec<Node>`/`Vec<Edge>` with no keyspace param since `KeyspaceState` *is* one keyspace).

`workspace_root` moves onto the `GraphPortStore` trait but is not a new concept — `PetgraphStore` already exposes `pub fn workspace_root(&self) -> Option<&Path>` (`lib.rs:128-129`); this just widens an existing public accessor onto a trait instead of introducing new state.

### 3.2 `EnrichEngine`

```rust
// cfdb-enrich::engine

pub struct EnrichEngine<'s, S> { store: &'s mut S }

impl<'s, S: GraphPortStore> EnrichEngine<'s, S> {
    pub fn new(store: &'s mut S) -> Self { Self { store } }

    fn require_workspace(&self, verb: &'static str, purpose: &str) -> Result<PathBuf, EnrichReport> {
        // identical logic to today's enrich_backend.rs:39-57, moved verbatim
    }
}

impl<'s, S: GraphPortStore> EnrichBackend for EnrichEngine<'s, S> {
    fn enrich_rfc_docs(&mut self, keyspace: &Keyspace) -> Result<EnrichReport, StoreError> {
        let root = match self.require_workspace("enrich_rfc_docs", "...") { Ok(r) => r, Err(rep) => return Ok(rep) };
        let port = self.store.graph_port(keyspace)?;   // guard #1 (UnknownKeyspace) lives inside graph_port
        Ok(rfc_docs::run(port, &root))
    }
    // ...same shape for the other 6 verbs
}
```

`require_keyspace` (guard #1, `enrich_backend.rs:26-31`) moves *into* `GraphPortStore::graph_port`'s implementation on `PetgraphStore` — it was always "does the store know this keyspace," which is exactly what resolving a `GraphPort` for it answers. `require_workspace` (guard #2) moves onto `EnrichEngine` verbatim — it is pure logic over `Option<PathBuf>`, no petgraph dependency, already.

`enrich_reachability`'s two-pass orchestration (`enrich_backend.rs:144-178`, All then ProductionOnly BFS) and `enrich_deprecation`'s always-`not_implemented`-report-with-a-real-warning shape (`enrich_backend.rs:60-77`, no port access needed at all) move verbatim into `EnrichEngine` — no logic change, just a new home.

### 3.3 Reuse, not a new enum

`Direction` for `GraphPort::neighbors` reuses `cfdb_core::query::ast::Direction` (`Out`/`In`/`Undirected`, already public, already the query-DSL's own vocabulary for edge direction — `query/ast.rs:114`), rather than inventing a second direction enum or vendoring `petgraph::Direction`'s `Outgoing`/`Incoming`. `Out`→`Outgoing`, `In`→`Incoming`; `Undirected` returns the union (no current pass needs it, but it costs nothing and avoids a partial enum in cfdb-core's own domain vocabulary — ddd-lens ubiquitous-language point, pre-trimmed before council).

### 3.4 Migration order (strangler-fig, one crate-boundary-crossing per slice)

| Slice | Pass | Why this slot |
|---|---|---|
| 056-0 | Port definition + `GraphPortStore`/`GraphPort` impl on `PetgraphStore`/`KeyspaceState`, `cfdb-enrich` crate scaffold + `EnrichEngine` shell, `enrich_deprecation` (trivial, no port use) rides along | Additive only — zero behavior change, zero pass moved yet. Proves the port compiles and delegates correctly before any real pass depends on it. |
| 056-A | `rfc_docs` (#107) | stdlib-only, no `NodeIndex` traversal — cheapest proof of the read+write port shape |
| 056-B | `bounded_context` (#108) | proves the port composing with the `cfdb-concepts` dependency |
| 056-C | `concepts` (#109) | proves `ingest_nodes`/`ingest_edges` node-creation path through the port |
| 056-D | `git_history` (#105) | feature-gated (`git-enrich`) — proves the feature flag survives the crate split |
| 056-E | `metrics` (#203) | feature-gated (`quality-metrics`), heaviest submodule tree — do after the pattern is proven 4 times |
| 056-F | `reachability` + `attr_call_resolution` (#110) | deepest coupling (BFS, `Direction`, `EdgeRef`) + best existing test coverage (20+ sites per prior session's matrix) — most protected, done last |
| 056-G | Cutover + cleanup | `cfdb-cli` composition root switches to `EnrichEngine`; delete `enrich_backend.rs` + `enrich/` from `cfdb-petgraph`; `cfdb-petgraph`'s `Cargo.toml` drops `syn`/`sha2`/`git2` + the `git-enrich`/`quality-metrics` features; dep-rules + CI updated |

Each slice A–F: move the pass module + its tests into `cfdb-enrich`, rewrite against `GraphPort`, add the `EnrichEngine` dispatch arm, delete the old arm from `cfdb-petgraph::enrich_backend.rs`. The PR #575 characterization test for that verb (plus 056-0's port-delegation tests) is the acceptance gate — must pass with **zero assertion changes**. Self-dogfood: `cfdb extract --workspace .` + the migrated `enrich-<verb>` on cfdb-self, diff report JSON against the pre-slice baseline (captured once at 056-0) — must be empty.

## 4. Invariants

- **No wire-format change. No `SchemaVersion` bump. No graph-specs lockstep.** `Node`/`Edge`/`EnrichReport` shapes untouched; this RFC moves code across a crate boundary behind a new trait, it does not add a fact type.
- **Determinism (G1) preserved by construction, not re-derived**: `GraphPort::nodes_with_label` must return ids in the same order `KeyspaceState::nodes_with_label` already returns `NodeIndex`es (sorted by `BTreeSet<NodeIndex>` iteration, `graph.rs:301-306`) — the port method is a direct id-resolution wrapper, not a reimplementation. Same for any other ordered read. A slice that changes iteration order is a bug, not a refactor.
- **Recall**: N/A — no extractor change, no new fact kind, `cfdb-recall` corpus untouched.
- **Behavior-identical, not merely "tests still pass"**: every migrated verb's `EnrichReport` (verb name, `ran`, all three counters, exact warning text) stays byte-identical pre/post-slice on every fixture the characterization suite (PR #575 + 056-0 additions) covers, **and** on a self-dogfood extract of cfdb's own tree (diff-empty, not eyeballed).
- **No-ratchet**: no baseline/allowlist file; nothing here introduces a threshold.

## 5. Architect lenses

*(filled by council review — see §7 process note; this RFC is not ratified until this section carries 4/4 RATIFY or a documented override)*

### 5.1 Clean architecture (`clean-arch`)

### 5.2 Domain-driven design (`ddd-specialist`)

### 5.3 SOLID / component principles (`solid-architect`)

### 5.4 Rust systems (`rust-systems`)

## 6. Non-goals

- **`eval/` extraction.** Identical coupling shape (`eval/mod.rs:113`: `pub(crate) state: &'a KeyspaceState`), deliberately out of scope — a fast-follow RFC once this pattern is proven across 7 real passes. Bundling it here would double the blast radius of an already-8-slice migration.
- **Correctness re-evaluation of any pass.** Feathers discipline: this RFC pins current behavior across a crate boundary. Whether `reachability`'s dual-pass shape, `metrics`'s regex-based complexity count, or any other pass's *behavior* is correct is an explicit, separate, later RFC.
- **Perf optimization of the port.** `GraphPort`'s id-based methods add a lookup indirection (`id → NodeIndex`) that today's direct `NodeIndex`-keyed access doesn't pay. Deferred — if self-dogfood timing regresses meaningfully during a slice, the fix is a follow-up issue against that slice, not a redesign of the port before shipping it.
- **GAT-based / associated-type keyspace views.** Rejected in §3.1 — no consumer needs it.
- **Vendoring a new `Direction` enum.** Rejected in §3.3 — reuses `cfdb_core::query::ast::Direction`.

## 7. Issue decomposition

See §3.4 table for the 8 slices and their ordering rationale. Per-slice `Tests:` blocks (§2.5 template; 056-0 is new-capability shaped, 056-A through 056-G are mechanical-refactor shaped per CLAUDE.md §2.5's own classification — "the existing suite must pass byte-identically" — plus the self-dogfood diff assertion restated every slice since it is the actual strangler-fig acceptance signal):

**056-0 — Port + `EnrichEngine` scaffold (additive, zero passes moved)**
```
Tests:
  - Unit: GraphPort delegation — node_by_id/nodes_with_label/neighbors/set_attr/ingest_nodes/ingest_edges
    on KeyspaceState produce identical results to calling the underlying pub(crate) methods directly,
    on a small synthetic fixture (a handful of nodes + edges, at least one multi-edge-label node).
  - Self dogfood (cfdb on cfdb): capture the 056-0 baseline — cfdb extract --workspace . +
    every enrich-<verb> report JSON on cfdb-self, saved as the diff target for slices A-F.
  - Cross dogfood: none — no behavior change, nothing to compare on the companion.
  - Target dogfood: none — rationale: zero passes moved yet, nothing observable changed on qbot-core.
```

**056-A through 056-F — one pass each (rfc_docs / bounded_context / concepts / git_history / metrics / reachability+attr_call_resolution)**
```
Tests:
  - Unit: none new — mechanical refactor (CLAUDE.md §2.5). The moved pass's existing unit tests
    (already characterized where present, e.g. metrics/tests.rs, git_history/tests.rs) move with it
    and must pass unchanged against the GraphPort-based rewrite.
  - Self dogfood (cfdb on cfdb): enrich-<verb> report JSON on cfdb-self diff-empty against the
    056-0 baseline. This is the slice's real acceptance gate, not the unit suite.
  - Cross dogfood (cfdb on graph-specs-rust at pinned SHA): ci/cross-dogfood.sh — 0 findings
    (no ban-rule or schema change in this RFC, so this is a regression check, not new coverage).
  - Target dogfood: none — rationale: internal port refactor, no observable capability change on
    qbot-core; the self-dogfood diff-empty assertion is the load-bearing signal for "nothing changed."
```

**056-G — Cutover + cleanup**
```
Tests:
  - Unit: none new — deletion + composition-root rewire.
  - Self dogfood (cfdb on cfdb): full `cfdb enrich-*` battery on cfdb-self, diff-empty against the
    056-0 baseline, run through the NEW cfdb-cli → EnrichEngine path (proves the composition-root
    rewire, not just the port, is behavior-preserving).
  - Cross dogfood: ci/cross-dogfood.sh — 0 findings.
  - Target dogfood: none — rationale: no observable capability change.
```

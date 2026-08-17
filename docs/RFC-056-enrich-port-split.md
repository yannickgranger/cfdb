# RFC-056 — `GraphBackend`: split `cfdb-petgraph`'s enrichment passes behind a port (strangler-fig)

- Status: **DRAFT — R1 folded, R2 pending** (2026-08-17) — clean-arch · ddd-specialist · solid-architect · rust-systems, all 4× REQUEST CHANGES on R1, folds below.
- Refs: cfdb self-audit 2026-08-17 (session `220c95c4`, artifact `cfdb Self-Audit`) — major finding: `cfdb-petgraph` is the repo's big-ball-of-mud. Characterization safety net: PR #575 (commit `c21f89b` on develop) pins the current `EnrichBackend` dispatch surface byte-for-byte. Precedent: RFC-031 (original `EnrichBackend` / `StoreBackend` port split).

## 1. Problem

`cfdb-petgraph` bundles three responsibilities that change for unrelated reasons and have **zero shared commit history** between two of them — independently re-verified by the solid-architect lens (`git log --oneline` on each path, `comm -12` on the sorted hash sets, empty result; 37 commits on `enrich/`, 31 on `eval/`): the `StoreBackend` adapter (petgraph storage), the Cypher evaluator (`eval/`), and 7 enrichment passes (`enrich/`, dispatched via `EnrichBackend`). CRP/CCP: three axes of change, one crate, one `Cargo.toml`, one compile unit.

The concrete coupling (verified at source, not asserted from memory):

- `KeyspaceState` (`crates/cfdb-petgraph/src/graph.rs:27`) is `pub(crate)` and holds `pub(crate) graph: StableDiGraph<Node, Edge>` (`graph.rs:30`) directly.
- All 7 enrichment `run()` functions take `&mut KeyspaceState` and reach straight into petgraph internals: `enrich/reachability.rs` imports `petgraph::stable_graph::NodeIndex`, `petgraph::visit::EdgeRef`, `petgraph::Direction` and walks `state.graph.edges_directed(idx, Direction::Outgoing)` directly (`reachability.rs:65-67,212,252-254`). `bounded_context.rs`, `concepts.rs`, `attr_call_resolution.rs` do the same for `NodeIndex`-keyed reads/writes (`node_weight_mut(idx).props.insert(...)`, `bounded_context.rs:163-164`).
- **A second, previously uncited petgraph-coupling site** (rust-systems R1 finding): `enrich/metrics/mod.rs:126`'s `FnItem` struct carries `node_idx: petgraph::stable_graph::NodeIndex` directly, consumed by `collect_fn_items` (`mod.rs:129-163`) and `apply_item_attrs` (`mod.rs:182-215`); `enrich/metrics/clustering.rs:83-93`'s test module also imports and constructs `NodeIndex` fixtures directly. Slice 056-E's actual diff is larger than "the metrics pass" alone — see §7.
- No port exists between enrichment *logic* and the petgraph *storage engine*. `eval/` has the identical shape (`eval/mod.rs:37,104`: imports `KeyspaceState`, holds `pub(crate) state: &'a KeyspaceState` — independently confirmed, solid R1) and is explicitly **out of scope** here (§6).

This is not "one big file" — it is an absent seam. A pass cannot be tested, changed, or reasoned about without the concrete petgraph representation in scope, and nothing in the type system stops a new pass from reaching further into `StableDiGraph` internals tomorrow.

**Why now, why this shape:** the prior session built a full 7-verb × 4-branch characterization matrix and shipped the 6 missing pinning tests + 1 CI step (PR #575) specifically so this extraction can be checked byte-for-byte against a known-good baseline. This RFC is the extraction plan; it does **not** re-judge whether any pass's current behavior is correct — that is an explicit, separate, later question (Michael Feathers discipline: characterize, then strangle, then re-evaluate).

## 2. Scope

Ships:

- **`GraphView` / `GraphBackend`** — two new traits in a new `cfdb_core::graph_port` module (§3.1 — deliberately not folded into `enrich.rs`; solid R1: `GraphView`'s audience is pass authors, `GraphBackend`'s is the composition root, conflating them in one file recreates in miniature the problem this RFC fixes at the crate level). Named to match the existing `StoreBackend`/`EnrichBackend` suffix convention (ddd R1 — `GraphPort`/`GraphPortStore` would have been a third, unexplained naming scheme for the same architectural role cfdb already calls "Backend"). `GraphView` is the per-keyspace read/write surface an enrichment pass needs; `GraphBackend` is the per-store factory that resolves a keyspace into a `GraphView` (mirrors the `require_keyspace` guard `enrich_backend.rs:26-31` already does once per verb call today).
- **`Direction`** moves to `cfdb_core::schema` (peer of `Label`/`EdgeLabel`, not a query-grammar type — ddd R1) with `cfdb_core::query::ast` re-exporting it (`pub use crate::schema::Direction;`) so `eval/pattern/path.rs`'s existing imports keep compiling unchanged. See §3.3.
- **`cfdb-enrich`** — new workspace crate. Depends on `cfdb-core` (+ `cfdb-concepts`, + feature-gated `git2`/`syn`/`sha2` — same optional deps `cfdb-petgraph` carries today for the same passes) as its **production** dependencies. `[dev-dependencies]` additionally carries `cfdb-petgraph = { path = "../cfdb-petgraph" }` (rust-systems R1 — the moved test suites need a concrete `GraphBackend` impl to run against, and `PetgraphStore` is the only one that exists; precedented exactly by `cfdb-petgraph`'s own `[dev-dependencies] cfdb-query`, and exempt from the CLEAN-3 gate since `tests/architecture_dep_rule.rs` only scans `[dependencies]`, never `[dev-dependencies]`). This is a real infra choice, not a shortcut: per CLAUDE.md §2.5 it beats hand-rolling a fake `GraphBackend` purely to avoid a dev-only edge. Holds all 7 pass modules, rewritten against `GraphView` — zero `petgraph` import possible in **production** code, because `cfdb-enrich`'s non-dev dependency set never includes the `petgraph` crate (compiler-enforced, not convention).
- **`EnrichEngine<S: GraphBackend>`** in `cfdb-enrich`, implementing `cfdb_core::enrich::EnrichBackend` generically over any `GraphBackend` implementor.
- `cfdb-petgraph` implements `GraphBackend` (`KeyspaceState` implements `GraphView` directly — thin delegation to its existing `pub(crate)` methods, now reached through a `pub` trait instead of raw field access).
- **Composition-root cutover is incremental, not deferred to a final slice** (clean-arch R1 + solid R1, both independently blocking on the original draft's sequencing — see below). Each of slices 056-A through 056-F flips **that slice's verb** in `crates/cfdb-cli/src/enrich.rs`'s dispatcher (the sole production call site, confirmed by both lenses via full-workspace grep — `enrich.rs:41-51`) from `store.enrich_x(&ks)` to `EnrichEngine::new(&mut store).enrich_x(&ks)` **in the same PR** that deletes the corresponding arm from `cfdb-petgraph::enrich_backend.rs`. 056-G no longer performs any behavioral cutover — it is pure deletion of now-dead code (`enrich_backend.rs`, `enrich/`) plus `Cargo.toml`/dep-rule cleanup once all 7 verbs have already moved.

  **Why this matters (the R1 blocker, both lenses independently found it):** `EnrichBackend`'s default trait methods fall back to `EnrichReport::not_implemented(...)` (`cfdb-core/src/enrich.rs:86-177`). Under the original "cutover only at G" sequencing, slice 056-A deleting `PetgraphStore::enrich_rfc_docs`'s override while `cfdb-cli` still calls `store.enrich_rfc_docs(&ks)` on that same `PetgraphStore` would silently regress the shipped `cfdb enrich-rfc-docs` CLI verb to "not implemented" for the entire window between 056-A and 056-G landing (5-6 separate merges, per this repo's per-issue-PR workflow) — and would break the existing self-dogfood tests (`crates/cfdb-cli/tests/self_dogfood_enrich_rfc_docs.rs` etc.) that call `store.enrich_x(&ks)` directly, which the RFC's own §4 invariant (byte-identical, self-dogfood diff-empty) cannot then satisfy. Per-slice cutover closes this: at every point on `develop`, exactly one `EnrichBackend` implementor is live per verb, and it is always the currently-correct one.
- `crates/cfdb-cli/Cargo.toml` gains feature-forwarding to `cfdb-enrich` for `git-enrich`/`quality-metrics`/`llvm-cov` (today forwarded straight to `cfdb-petgraph`, `Cargo.toml:98,102,105`) — introduced incrementally: 056-D adds `git-enrich` forwarding to `cfdb-enrich` (petgraph's own `git-enrich` forwarding becomes dead but harmless until 056-G prunes it), 056-E does the same for `quality-metrics`/`llvm-cov`. Mid-migration (post-D, pre-E) the two flags forward to different crates — call this out explicitly in each PR body, not silently.
- `.cfdb/workspace-dep-rules.toml` gains a `[cfdb-enrich]` section (allowed: `cfdb-core`, `cfdb-concepts`, feature-gated `git2`/`syn`/`sha2`; forbidden: `cfdb-petgraph` and every crate already forbidden to `cfdb-petgraph`, same non-cycle rationale). Per the file's own stated convention (tripwire = "any crate whose presence would invert the Dependency Rule or create a cycle," `workspace-dep-rules.toml:18-22`), `[cfdb-core].forbidden` and `[cfdb-petgraph].forbidden` both gain `cfdb-enrich` (clean-arch R1 finding 2 — verified absent today via grep, `exit 1`). `[cfdb-petgraph].allowed` drops `syn`/`sha2`/`git2` **and** `cfdb-concepts` once the last consumer of each moves out (`cfdb-concepts` is used only inside `enrich/bounded_context.rs` + `enrich/concepts.rs` today, confirmed zero non-enrich references — solid R1 finding 4; drops once 056-B/C land, not held to 056-G).
- `.gitea/workflows/ci.yml` gains a slim/full test matrix entry for `cfdb-enrich` mirroring the one just added for `cfdb-petgraph` (git-enrich / quality-metrics off-by-default).

Does **not** ship (see §6): any change to `eval/`; any change to Node/Edge/`:Concept`/`:RfcDoc` wire shape; any `SchemaVersion` bump; any behavior change to any of the 7 passes — the acceptance bar for every slice is "characterization tests + self-dogfood diff are byte-identical to the pre-slice baseline" (with one named, measured exception for 056-F's traversal cost — §6, §7).

## 3. Design

### 3.1 The port

```rust
// cfdb-core::graph_port (new sibling module — solid R1: not enrich.rs)

pub trait GraphView {
    fn node_by_id(&self, id: &str) -> Option<&Node>;
    fn nodes_with_label(&self, label: &Label) -> Vec<String>;      // ids, sorted — see §4 determinism
    fn neighbors(&self, id: &str, dir: Direction) -> Vec<(EdgeLabel, String)>; // (edge label, other-endpoint id)
    fn set_attr(&mut self, id: &str, key: &str, value: PropValue) -> bool;    // false = unknown id
    fn ingest_nodes(&mut self, nodes: Vec<Node>);
    fn ingest_edges(&mut self, edges: Vec<Edge>);
}

pub trait GraphBackend: Send + Sync {
    fn graph_view(&mut self, keyspace: &Keyspace) -> Result<&mut dyn GraphView, StoreError>;
    fn workspace_root(&self) -> Option<&Path>;
}
```

`GraphBackend: Send + Sync` is required, not optional (rust-systems R1, compile-blocking as originally drafted without it): `cfdb_core::enrich::EnrichBackend: Send + Sync` already (`enrich.rs:77`, mirroring `StoreBackend: Send + Sync`, `store.rs:61`), and §3.2's `impl<'s, S: GraphBackend> EnrichBackend for EnrichEngine<'s, S>` must prove `EnrichEngine<'s, S>: Send + Sync` — its only field `&'s mut S` makes that conditional on `S: Send + Sync`, unprovable for an unconstrained `S`. `PetgraphStore` already satisfies this transitively, so the bound costs nothing at the one real call site.

`GraphView` is dyn-safe on purpose — no generics, no GATs. A "per-keyspace typed view" design (associated-type keyspace handle) was considered and rejected: it buys nothing here (every pass resolves one keyspace once, per today's `enrich_backend.rs` guard pattern) and would be the first GAT in the workspace for no consumer benefit (rust-systems trim, pre-council). `GraphBackend::graph_view`'s `&mut dyn GraphView` return needs no explicit lifetime parameter — elision rule 3 ties the elided output lifetime to `&mut self`, same shape as any `fn as_mut(&mut self) -> &mut dyn Trait` (rust-systems R1, confirmed compiles).

`node_by_id` / `nodes_with_label` / `neighbors` map onto `KeyspaceState`'s existing `pub(crate)` methods almost verbatim: `nodes_with_label` already exists (`graph.rs:297-302`, returns sorted `NodeIndex`es — the port version resolves each to its `id` before returning, same order); `neighbors` replaces the direct `state.graph.edges_directed(idx, dir)` calls in `reachability.rs`/`attr_call_resolution.rs`/`bounded_context.rs`; `set_attr` replaces the direct `node_weight_mut(idx).props.insert(...)` pattern (`bounded_context.rs:163-164`); `ingest_nodes`/`ingest_edges` are literal 1:1 forwards to `KeyspaceState::ingest_nodes`/`ingest_edges` (`graph.rs:118,260` — already exist, already take `Vec<Node>`/`Vec<Edge>` with no keyspace param since `KeyspaceState` *is* one keyspace) and cannot bypass `by_label` maintenance, `by_prop` inverted-index reconciliation, or RFC-054 identity-contention recording — those all live inside `ingest_one_node`/`ingest_one_edge`, the sole callees, confirmed at source (rust-systems R1).

`workspace_root` moves onto the `GraphBackend` trait but is not a new concept — `PetgraphStore` already exposes `pub fn workspace_root(&self) -> Option<&Path>` (`lib.rs:128-129`); this just widens an existing public accessor onto a trait instead of introducing new state.

### 3.2 `EnrichEngine`

```rust
// cfdb-enrich::engine

pub struct EnrichEngine<'s, S> { store: &'s mut S }

impl<'s, S: GraphBackend> EnrichEngine<'s, S> {
    pub fn new(store: &'s mut S) -> Self { Self { store } }

    fn require_workspace(&self, verb: &'static str, purpose: &str) -> Result<PathBuf, EnrichReport> {
        // identical logic to today's enrich_backend.rs:39-57, moved verbatim
    }
}

impl<'s, S: GraphBackend> EnrichBackend for EnrichEngine<'s, S> {
    fn enrich_rfc_docs(&mut self, keyspace: &Keyspace) -> Result<EnrichReport, StoreError> {
        let root = match self.require_workspace("enrich_rfc_docs", "...") { Ok(r) => r, Err(rep) => return Ok(rep) };
        let view = self.store.graph_view(keyspace)?;   // guard #1 (UnknownKeyspace) lives inside graph_view
        Ok(rfc_docs::run(view, &root))
    }
    // ...same shape for the other 6 verbs
}
```

`require_keyspace` (guard #1, `enrich_backend.rs:26-31`) moves *into* `GraphBackend::graph_view`'s implementation on `PetgraphStore` — it was always "does the store know this keyspace," which is exactly what resolving a `GraphView` for it answers. `require_workspace` (guard #2) moves onto `EnrichEngine` verbatim — it is pure logic over `Option<PathBuf>`, no petgraph dependency, already. No pass-level logic (BFS/TOML/regex/syn) moves into `EnrichEngine` itself — it stays pure dispatch + the two pre-existing guards (solid R1, SRP confirmed clean; boundary rule stated here explicitly so future slices don't grow logic into it).

`enrich_reachability`'s two-pass orchestration (`enrich_backend.rs:144-178`, All then ProductionOnly BFS) and `enrich_deprecation`'s always-`not_implemented`-report-with-a-real-warning shape (`enrich_backend.rs:60-77`, no port access needed at all) move verbatim into `EnrichEngine` — no logic change, just a new home.

`EnrichEngine<'s, S>`'s test suites (the moved `enrich/{reachability,metrics,git_history,rfc_docs,concepts}/tests.rs`, 880/49/292/283/356 lines respectively) run against `S = PetgraphStore` via `cfdb-enrich`'s `[dev-dependencies]` edge (§2). One exception: `attr_call_resolution.rs`'s 5 inline unit tests (lines 245/267/277/291/303) construct a bare `KeyspaceState::new()` and probe `id_to_idx`/`resolve_callee_to_item` directly, bypassing `PetgraphStore` entirely — these do **not** move verbatim (correcting the original draft's blanket "existing tests move unchanged" claim, rust-systems R1); they get rewritten against `&dyn GraphView` as part of 056-F, which is a small, mechanical rewrite (the only stateful operation is two `id_to_idx.get` probes — exactly what `node_by_id` replaces).

### 3.3 Reuse, not a new enum — and the right home for it

`Direction` for `GraphView::neighbors` reuses the existing enum at (pre-move) `cfdb_core::query::ast::Direction` (`Out`/`In`/`Undirected`) rather than inventing a second direction enum or vendoring `petgraph::Direction`'s `Outgoing`/`Incoming`. This reuse decision is *correct* (ddd R1, confirmed independently against `eval/pattern/path.rs:183-186,236-239`, where the evaluator already treats `Direction` as a **runtime traversal instruction** — `Direction::Undirected => self.collect_directed_edges(idx, edge, true, true)` already means "union both directions" at actual graph-walk time, the exact semantic `GraphView::neighbors`'s `Undirected` needs — this is one shared concept across the query-pattern and enrichment-traversal contexts, not a coincidental homonym).

What was wrong in the original draft was the enum's **location**: `query::ast`'s module doc scopes it explicitly to "AST node types... produced by the parser/builder and consumed by the store evaluator" (`ast.rs:1-7`). Once `cfdb-enrich` — a context with nothing to do with Cypher grammar — imports `Direction` from there, that module doc becomes false and every enrichment-pass file reads as pulling a piece of the query language into an enrichment pass. `Direction`'s true home is graph-topology vocabulary, a peer of `Label`/`EdgeLabel` (`cfdb_core::schema`, already imported by `fact.rs:12`) — so it moves to `cfdb_core::schema`, with `query::ast` re-exporting it (`pub use crate::schema::Direction;`) so `eval/pattern/path.rs`'s existing imports need zero changes. Zero behavior change, one compiler-enforced move, lands in 056-0.

### 3.4 Migration order (strangler-fig, one crate-boundary-crossing per slice, composition root moves WITH each pass)

| Slice | Pass | Composition-root edit (same PR) | Why this slot |
|---|---|---|---|
| 056-0 | Port + `GraphBackend`/`GraphView` in `cfdb_core::graph_port`, `Direction` relocation (§3.3), `cfdb-enrich` scaffold + `EnrichEngine` shell + `[dev-dependencies] cfdb-petgraph` wiring, `enrich_deprecation` (trivial, no port use) rides along | none — `cfdb-cli` still calls `PetgraphStore` for every verb | Additive only — zero behavior change, zero pass moved yet. Proves the port compiles and delegates correctly before any real pass depends on it. |
| 056-A | `rfc_docs` (#107) | `EnrichVerb::RfcDocs` arm → `EnrichEngine` | stdlib-only, no `NodeIndex` traversal — cheapest proof of the read+write port shape |
| 056-B | `bounded_context` (#108) | `EnrichVerb::BoundedContext` arm → `EnrichEngine` | proves the port composing with the `cfdb-concepts` dependency |
| 056-C | `concepts` (#109) | `EnrichVerb::Concepts` arm → `EnrichEngine` | proves `ingest_nodes`/`ingest_edges` node-creation path through the port; `cfdb-concepts` drops from `cfdb-petgraph`'s deps once B+C both land |
| 056-D | `git_history` (#105) | `EnrichVerb::GitHistory` arm → `EnrichEngine`; `cfdb-cli` gains `git-enrich` forwarding to `cfdb-enrich` | feature-gated (`git-enrich`) — proves the feature flag survives the crate split |
| 056-E | `metrics` (#203) — **larger than one file**: also `metrics/mod.rs:126`'s `FnItem.node_idx: NodeIndex` (→ id `String`) and `clustering.rs:83-93`'s `NodeIndex` test fixtures | `EnrichVerb::Metrics` arm → `EnrichEngine`; `cfdb-cli` gains `quality-metrics`/`llvm-cov` forwarding to `cfdb-enrich` | feature-gated (`quality-metrics`), heaviest submodule tree — do after the pattern is proven 4 times |
| 056-F | `reachability` + `attr_call_resolution` — **including the 5 inline unit tests that bypass `PetgraphStore`, rewritten not moved (§3.2)** | `EnrichVerb::Reachability` arm → `EnrichEngine` | deepest coupling (BFS, `Direction`, `EdgeRef`) + best existing test coverage (20+ sites per prior session's matrix) — most protected, done last. **Named perf exception — see §6/§7**: the ported BFS walks ~246k `CALLS` edges on cfdb-self; today's `bfs_call_graph` (`reachability.rs:244-260`) uses zero-alloc `Copy`-typed `u32`-backed `NodeIndex` in a `BTreeSet`/`VecDeque`; the id-based port forces `String`-keyed visited-sets and a fresh allocation per queue pop. This is a real allocation-class change (not the "one extra hash lookup" class the other 5 passes pay), so 056-F's acceptance gate includes a measured self-dogfood wall-clock/allocation comparison, not diff-empty correctness alone. |
| 056-G | Cutover cleanup (no behavior change — all 7 verbs already moved) | none — already done incrementally | Delete `enrich_backend.rs` + `enrich/` from `cfdb-petgraph`; `Cargo.toml` drops `syn`/`sha2`/`git2` + the `git-enrich`/`quality-metrics` features (already unused by this point); dep-rules `[cfdb-core]`/`[cfdb-petgraph]` forbidden-list tripwires added; CI matrix updated |

Each slice A–F: move the pass module + its tests into `cfdb-enrich`, rewrite against `GraphView`, add the `EnrichEngine` dispatch arm, delete the old arm from `cfdb-petgraph::enrich_backend.rs`, **and flip that verb's `cfdb-cli/src/enrich.rs` dispatch arm in the same PR** (§2). The PR #575 characterization test for that verb (plus 056-0's port-delegation tests) is the acceptance gate — must pass with **zero assertion changes**. Self-dogfood: `cfdb extract --workspace .` + the migrated `enrich-<verb>` **run through the real `cfdb` binary** (`tools/dogfood-enrich` drives exactly this, subprocess-invoking the shipped binary — confirmed at `tools/dogfood-enrich/src/runner.rs`) on cfdb-self, diff report JSON against the pre-slice baseline (captured once at 056-0) — must be empty, except 056-F's named perf measurement above.

## 4. Invariants

- **No wire-format change. No `SchemaVersion` bump. No graph-specs lockstep.** `Node`/`Edge`/`EnrichReport` shapes untouched; this RFC moves code across a crate boundary behind a new trait, it does not add a fact type.
- **Determinism (G1) preserved by construction, not re-derived**: `GraphView::nodes_with_label` must return ids in the same order `KeyspaceState::nodes_with_label` already returns `NodeIndex`es (sorted by `BTreeSet<NodeIndex>` iteration, `graph.rs:297-302`) — the port method is a direct id-resolution wrapper, not a reimplementation. Same for any other ordered read. A slice that changes iteration order is a bug, not a refactor.
- **Recall**: N/A — no extractor change, no new fact kind, `cfdb-recall` corpus untouched.
- **Behavior-identical, not merely "tests still pass"**: every migrated verb's `EnrichReport` (verb name, `ran`, all three counters, exact warning text) stays byte-identical pre/post-slice on every fixture the characterization suite (PR #575 + 056-0 additions) covers, **and** on a self-dogfood extract of cfdb's own tree run through the real `cfdb` binary (diff-empty, not eyeballed) — **and** at every point on `develop`, exactly one live `EnrichBackend` implementor exists per verb (§2 — no dead-stub regression window).
- **No-ratchet**: no baseline/allowlist file; nothing here introduces a threshold. 056-F's perf measurement (§3.4) is recorded in the PR body as evidence, not codified as a new gate/threshold file.

## 5. Architect lenses

R1 (all four independently reviewed the initial draft against source, not against each other):

### 5.1 Clean architecture (`clean-arch`) — R1: REQUEST CHANGES → folds applied above

Confirmed clean: port purity (`GraphView`/`GraphBackend` use only domain types, zero petgraph leakage — an encapsulation improvement over today's direct `pub(crate)` field reach-in), composition-root confinement to the single production call site, ISP preservation (`StoreBackend`-only consumers never see `graph_view`), no transient reverse crate-dependency edge at any slice. **Blocking finding, folded**: §2 Scope and §3.4's migration table originally disagreed on when `cfdb-petgraph`'s old `EnrichBackend` arms are deleted vs. when `cfdb-cli`'s composition root switches — as drafted, slices 056-A–F would delete the old arm per-slice while the CLI kept calling the (now-stubbed) old implementor until 056-G, a live regression window on the shipped binary. Fixed: composition-root cutover now moves with each slice (§2, §3.4 table). **Non-blocking, folded**: dep-rules tripwire completeness (`[cfdb-core]`/`[cfdb-petgraph]` forbidden lists gain `cfdb-enrich`, §2); RFC line-citation drift corrected against actual source line numbers throughout this revision.

### 5.2 Domain-driven design (`ddd-specialist`) — R1: REQUEST CHANGES → folds applied above

Confirmed clean: `Direction` reuse is the *correct* call (genuine shared concept, not a homonym — independently verified against `eval/pattern/path.rs`'s existing runtime semantics); `set_attr`'s prop/attr vocabulary split is consistent with existing usage; `cfdb-enrich`/`EnrichEngine` naming is coherent with `cfdb-core::enrich`'s existing vocabulary; `KeyspaceState implements GraphView` is a clean Anti-Corruption Layer, no Shared-Kernel risk between `cfdb-enrich` and `cfdb-petgraph`. **Folded**: (1) `Direction` relocates to `cfdb_core::schema` (peer of `Label`/`EdgeLabel`) rather than staying in `query::ast`, whose module doc scopes it to the query grammar only — re-exported from `query::ast` for zero-diff compat (§3.3). (2) `GraphPort`/`GraphPortStore` renamed to `GraphView`/`GraphBackend` — matches the existing `StoreBackend`/`EnrichBackend` suffix convention instead of introducing an unexplained third naming scheme for the same architectural role (§2, §3.1).

### 5.3 SOLID / component principles (`solid-architect`) — R1: REQUEST CHANGES → folds applied above

Independently re-verified both foundational claims: the §1 git-history zero-shared-commits claim (true, `comm -12` empty) and the "`eval/` has identical coupling shape" claim (true, `eval/mod.rs:37,104`). Confirmed clean: SDP direction preserved on `cfdb-core` (Ce=0 before/after); 056-F's pass grouping (`reachability` + `attr_call_resolution`) is real CCP-correct coupling, not arbitrary (`reachability.rs:154` calls into `attr_call_resolution` directly); `EnrichEngine` SRP is clean, no pass-level logic risk (boundary rule now stated explicitly, §3.2); ISP improvement is the RFC's strongest, quantified result (9 `pub(crate)` methods + 3 raw petgraph-typed fields today → 6 dyn-safe id-based methods, zero petgraph types in any pass signature). **Blocking finding, folded** (independently converged with clean-arch's finding): same composition-root sequencing bug, same fix. **Folded**: port-trait module placement now mandated as `cfdb_core::graph_port`, not left ambiguous between that and `enrich.rs` (§3.1); `cfdb-concepts` added to the Cargo.toml/dep-rules cleanup list, leaving `cfdb-petgraph` once 056-B/C land, not held to 056-G (§2). **Named, not folded as a fix (documented per solid's request)**: cfdb-core's pre-existing, RFC-031-precedented CRP softness (only 2/12 intra-workspace consumers use any port-trait surface; this RFC extends the pattern, doesn't introduce it) is recorded as a deferred non-goal (§6) rather than fixed here.

### 5.4 Rust systems (`rust-systems`) — R1: REQUEST CHANGES → folds applied above

Confirmed clean: `GraphView` dyn-safety (no generics/Self-returns/associated consts); `GraphBackend::graph_view`'s lifetime elision (compiles with no explicit lifetime param, same shape as `fn as_mut`); `ingest_nodes`/`ingest_edges` cannot bypass `by_label`/`by_prop`/RFC-054 contention-recording side effects (sole callees, confirmed at source); GAT rejection (§3.1) correct. **Blocking finding, folded**: `GraphBackend` was missing `Send + Sync` — `EnrichEngine<'s, S>: Send + Sync` (required by `EnrichBackend: Send + Sync`) is unprovable for unconstrained `S` without it; added (§3.1), costs nothing since `PetgraphStore` already satisfies it. **Folded**: (1) `metrics/mod.rs:126`'s `FnItem.node_idx` + `clustering.rs:83-93`'s test fixtures named as a second petgraph-coupling site in 056-E, not previously cited (§1, §3.4). (2) `cfdb-enrich`'s test suites need a concrete `GraphBackend` to run against — `[dev-dependencies] cfdb-petgraph`, precedented, exempt from CLEAN-3 (§2, §3.2). (3) `attr_call_resolution.rs`'s 5 inline unit tests bypass `PetgraphStore` and need an actual rewrite in 056-F, not a verbatim move — corrected (§3.2, §3.4, §7). (4) 056-F's BFS traversal is a genuine allocation-class regression (`Copy` `u32` `NodeIndex` → `String`-keyed), not the "one extra hash lookup" class the other passes pay — 056-F's acceptance gate now includes a measured comparison, not diff-empty alone (§3.4, §6, §7). (5) `cfdb-cli`'s feature-forwarding Cargo.toml edits named explicitly per-slice for 056-D/E rather than left to an undifferentiated final cutover (§2, §3.4).

## 6. Non-goals

- **`eval/` extraction.** Identical coupling shape (`eval/mod.rs:37,104`, independently confirmed by solid R1), deliberately out of scope — a fast-follow RFC once this pattern is proven across 7 real passes. Bundling it here would double the blast radius of an already-8-slice migration.
- **Correctness re-evaluation of any pass.** Feathers discipline: this RFC pins current behavior across a crate boundary. Whether `reachability`'s dual-pass shape, `metrics`'s regex-based complexity count, or any other pass's *behavior* is correct is an explicit, separate, later RFC.
- **Perf optimization of the port — with one named exception.** `GraphView`'s id-based methods add a lookup indirection (`id → NodeIndex`) that today's direct `NodeIndex`-keyed access doesn't pay; for 5 of the 6 remaining passes (rfc_docs, bounded_context, concepts, git_history, metrics — all node-attribute-scan shaped) this is genuinely "one extra hash lookup per item," deferred as stated. **Exception**: 056-F's reachability BFS is a real allocation-class change (§3.4, §7), not the same cost class — it gets a measured self-dogfood comparison as part of its own acceptance gate, not a blanket "deferred, fix later if it regresses."
- **GAT-based / associated-type keyspace views.** Rejected in §3.1 — no consumer needs it.
- **Vendoring a new `Direction` enum.** Rejected in §3.3 — reuses (and relocates) `cfdb_core::schema::Direction`.
- **Further CRP splitting of `cfdb-core`'s port-trait family from its data-model types.** Named, not fixed, per solid R1: only 2 of 12 intra-workspace consumers of `cfdb-core` use any port trait (`StoreBackend`/`EnrichBackend`) today; this RFC extends that existing pattern (2 traits → 4) rather than introducing it (RFC-031 precedent), and does not attempt to further separate `cfdb-core`'s port-trait surface from its DTO surface. A future RFC's problem, if the ratio ever becomes load-bearing.

## 7. Issue decomposition

See §3.4 table for the 8 slices, their composition-root wiring, and ordering rationale. Per-slice `Tests:` blocks (§2.5 template; 056-0 is new-capability shaped, 056-A through 056-G are mechanical-refactor shaped per CLAUDE.md §2.5's own classification — "the existing suite must pass byte-identically" — plus the self-dogfood diff assertion restated every slice since it is the actual strangler-fig acceptance signal, run through the real `cfdb` binary per `tools/dogfood-enrich`):

**056-0 — Port + `EnrichEngine` scaffold (additive, zero passes moved)**
```
Tests:
  - Unit: GraphView delegation — node_by_id/nodes_with_label/neighbors/set_attr/ingest_nodes/ingest_edges
    on KeyspaceState produce identical results to calling the underlying pub(crate) methods directly,
    on a small synthetic fixture (a handful of nodes + edges, at least one multi-edge-label node).
    GraphBackend: Send + Sync compiles for PetgraphStore; EnrichEngine<PetgraphStore>: Send + Sync
    compiles (the R1 rust-systems blocking finding — pin it as a compile-only test so a future
    change to GraphBackend's bound can't silently regress it). Direction relocation: eval/pattern/path.rs
    compiles unchanged against the cfdb_core::schema::Direction re-export from query::ast.
  - Self dogfood (cfdb on cfdb): capture the 056-0 baseline — cfdb extract --workspace . +
    every enrich-<verb> report JSON on cfdb-self (via the real cfdb binary), saved as the diff
    target for slices A-F.
  - Cross dogfood: none — no behavior change, nothing to compare on the companion.
  - Target dogfood: none — rationale: zero passes moved yet, nothing observable changed on qbot-core.
```

**056-A through 056-D — one pass each (rfc_docs / bounded_context / concepts / git_history)**
```
Tests:
  - Unit: none new — mechanical refactor (CLAUDE.md §2.5). The moved pass's existing unit tests
    move with it and must pass unchanged against the GraphView-based rewrite.
  - Self dogfood (cfdb on cfdb): enrich-<verb> report JSON on cfdb-self, produced via the real
    cfdb binary (cfdb-cli's dispatcher for this verb now routes through EnrichEngine — this IS
    part of the slice, not deferred), diff-empty against the 056-0 baseline. This is the slice's
    real acceptance gate, not the unit suite.
  - Cross dogfood (cfdb on graph-specs-rust at pinned SHA): ci/cross-dogfood.sh — 0 findings
    (no ban-rule or schema change in this RFC, so this is a regression check, not new coverage).
  - Target dogfood: none — rationale: internal port refactor, no observable capability change on
    qbot-core; the self-dogfood diff-empty assertion is the load-bearing signal for "nothing changed."
```

**056-E — metrics**
```
Tests:
  - Unit: none new for the pass logic itself (mechanical refactor). New: FnItem.node_idx's
    NodeIndex -> String id rewrite (mod.rs:126, collect_fn_items, apply_item_attrs) and
    clustering.rs's NodeIndex test fixtures rewritten against GraphView — these are real code
    changes (not a verbatim move) and get the mechanical-refactor bar: existing assertions,
    same expected values, pass unchanged.
  - Self dogfood (cfdb on cfdb): enrich-metrics report JSON (quality-metrics feature on) via the
    real cfdb binary, diff-empty against the 056-0 baseline. cfdb-cli's quality-metrics/llvm-cov
    feature forwarding now points at cfdb-enrich (§2) — verify both a --features quality-metrics
    build and a default (feature-off, degraded-report) build.
  - Cross dogfood: ci/cross-dogfood.sh — 0 findings.
  - Target dogfood: none — rationale: internal port refactor, no observable capability change.
```

**056-F — reachability + attr_call_resolution**
```
Tests:
  - Unit: attr_call_resolution.rs's 5 inline unit tests (currently constructing bare
    KeyspaceState::new() and probing id_to_idx/resolve_callee_to_item directly) are rewritten
    against &dyn GraphView, not moved verbatim — same assertions, same expected outcomes.
    reachability's existing 20+-site test suite moves with the pass and passes unchanged.
  - Self dogfood (cfdb on cfdb): enrich-reachability report JSON via the real cfdb binary,
    diff-empty against the 056-0 baseline (both ReachabilityFilter::All and ::ProductionOnly
    passes). PLUS: a measured wall-clock and allocation-count comparison of the BFS
    (crate::enrich::reachability::bfs_call_graph equivalent) pre- vs post-migration on the
    cfdb-self keyspace (~246k CALLS edges) — recorded in the PR body as evidence per §3.4/§6's
    named perf exception. Not a new gate/threshold file (no-ratchet, §4) — a reviewer
    sanity-check number, same spirit as target-dogfood rows elsewhere in this repo's Tests
    template.
  - Cross dogfood: ci/cross-dogfood.sh — 0 findings.
  - Target dogfood: none — rationale: internal port refactor; the self-dogfood perf measurement
    above is the load-bearing signal for this slice specifically, superseding the usual
    "target dogfood: none" boilerplate reasoning used by the other slices.
```

**056-G — Cutover cleanup (no remaining behavioral cutover — already done incrementally)**
```
Tests:
  - Unit: none new — deletion only (enrich_backend.rs, enrich/ removed from cfdb-petgraph).
  - Self dogfood (cfdb on cfdb): full `cfdb enrich-*` battery on cfdb-self, diff-empty against
    the 056-0 baseline, confirming deletion of the dead PetgraphStore EnrichBackend impl didn't
    change anything observable (it shouldn't — cfdb-cli stopped calling it in 056-A..F).
  - Cross dogfood: ci/cross-dogfood.sh — 0 findings.
  - Target dogfood: none — rationale: no observable capability change; pure cleanup.
```

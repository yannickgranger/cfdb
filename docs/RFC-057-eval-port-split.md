# RFC-057 — `GraphReader`: split `cfdb-petgraph`'s Cypher evaluator behind a port (strangler-fig, fast-follow to RFC-056)

- Status: **council 4/4 RATIFY (2026-08-18, R2)**. **RATIFIED on merge to doxa `develop` by the operator.** Amendments: council synthesis, operator merge.
- Refs: `cfdb-056-enrich-port-split#6` (names `eval/` extraction as the fast-follow once the `GraphView` pattern is proven — it now is: 056-0 through 056-G merged, `cfdb-enrich::EnrichEngine` is the sole `EnrichBackend` implementor, `cfdb-petgraph` is storage + evaluator only, develop `4e6018c`). cfdb self-audit 2026-08-17 (artifact `cfdb Self-Audit`, "Major: `cfdb-petgraph` bundles two components that have never once changed together"). Precedent: cfdb-031-audit-cleanup (`EnrichBackend`/`StoreBackend` port split), cfdb-035-persistent-inverted-indexes#4 (`execute_explained` kept off `StoreBackend`), cfdb-054-target-identity-namespace#3.4 (ingest-warning prepend on every `execute`).

## 1. Problem

After cfdb-056-enrich-port-split, `cfdb-petgraph` (9,615 LOC, still 1.4× the next-largest crate) holds two responsibilities with the same absent seam cfdb-056-enrich-port-split closed for enrichment: the petgraph **storage engine** (`graph.rs`, `index/`, `persist.rs`, `ingest_contention.rs`, `canonical_dump.rs`) and the **Cypher evaluator** (`eval/`, 2,252 non-test LOC / 81 fns across 10 files, ~1,480 test LOC). Co-change over the crate's full history (`git log` per path, `comm -12` on sorted hash sets, measured 2026-08-18 on develop `4e6018c`; independently re-run by solid R1, exact match): 32 commits touch `eval/`, 36 touch the storage files, **6 shared** — of which 3 are repo-wide sweeps (comment-doctrine strip `3eb9421`, quality-metrics drain `9b5e93d`, `#[non_exhaustive]` gate `f36e794`) and 1 is the initial portage `8ed8b97`. Two genuine co-changes in the crate's life (`7ce3c71` rebuild-on-load + `last_segment`, `e9197a3` indexes.toml wiring). Same CCP evidence class cfdb-056-enrich-port-split#1 recorded for `enrich/` (37 vs 31, zero shared).

The concrete coupling (verified at source on develop `4e6018c`, not from memory; every citation below independently re-verified by clean-arch R1):

- `crates/cfdb-petgraph/src/eval/mod.rs:103-104` — `pub(crate) struct Evaluator<'a> { pub(crate) state: &'a KeyspaceState, … }` — the evaluator holds the concrete storage struct, not a trait object. `KeyspaceState` (`graph.rs:27-88`) is `pub(crate)` with every field `pub(crate)`: `graph: StableDiGraph<Node, Edge>` (:30), `id_to_idx` (:34), `by_label` (:39), `edge_labels` (:43), `by_prop` (:77), `indexed_pairs` (:87).
- **The binding table is petgraph-typed at its core**: `eval/mod.rs:67-85` `enum Binding { NodeRef(NodeIndex), EdgeRef(EdgeIndex), Value(RowValue), Null }` — two of four variants are raw `petgraph::stable_graph` handles; `Bindings = BTreeMap<String, Binding>` (:88) and `BindingStream` (:94) carry them through every MATCH/WHERE/WITH/RETURN stage.
- Direct reach-ins, per file: `eval/pattern.rs:147,150,174,177` (`has_label`, `by_label.keys()`, `nodes_with_label`, `all_nodes_sorted`), `:163-169` (`index::lookup::candidates_from_index(state, …)`), `:185-196` (`bound_var_index_value` calls the store-internal `crate::index::build::index_key_of` directly — an existing ACL leak, see §3.1), `:194,199` (`state.graph[idx].props` / `state.graph[idx]`); `eval/pattern/coupling.rs:74,107,147` (three free fns take `&KeyspaceState`), `:161-166` (`state.by_prop.get(..)` emptiness probe — the measured 625M-pair → single-label-scan hoist, `coupling.rs:52-59`); `eval/pattern/path.rs:41,46` (`has_edge_label`, `edge_labels.iter()`), `:145` (`state.graph[idx].label`), `:190` (`EdgeIndex::index()` as the determinism sort key), `:251-275` (`collect_directed_edges`: `state.graph.edges(idx)` / `edges_directed(idx, petgraph::Direction::Incoming)` + `EdgeRef::{target,source,id,weight}`); `eval/predicate.rs:63,65,85,86-90` (`state.graph[idx].props`, `.id`, `graph.edge_weight(idx)`), `:45` (re-entrant `Evaluator::new(self.state, …)` for `NOT EXISTS`).
- The **output** side is already clean: `cfdb_core::result::RowValue` (`result.rs:25-28`) is `Scalar(PropValue) | List(Vec<PropValue>)`; every deref to a concrete value happens at `eval/return_clause.rs:131` and `eval/with_clause.rs:31,142,148`. Five of ten eval files (`util.rs`, `with_clause.rs`, `return_clause.rs`, `predicate/udf.rs`, `explain_fmt.rs`) reference neither `petgraph` nor `KeyspaceState` today.
- Only two production entry points into `eval/`, both inside `cfdb-petgraph/src/lib.rs`: `impl StoreBackend for PetgraphStore { fn execute }` (`lib.rs:222-231`) and the inherent `PetgraphStore::execute_explained` (`lib.rs:182-198`). Both do the same three things: resolve the keyspace (`UnknownKeyspace` guard), run the `Evaluator`, and **prepend `state.materialized_ingest_warnings()`** to `result.warnings` (cfdb-054-target-identity-namespace#3.4). No other crate names `Evaluator`; `cfdb-cli` reaches the explain path only through `scope/explain_sink.rs:54` and imports `cfdb_petgraph::explain::ExplainRow` at `explain_sink.rs:17`.

cfdb-056-enrich-port-split#6 deferred exactly this ("identical coupling shape … a fast-follow RFC once this pattern is proven across 7 real passes") to keep that migration's blast radius bounded. The pattern is now proven; the seam is still absent; nothing in the type system stops a new evaluator feature from reaching further into `StableDiGraph` internals tomorrow.

**What is different from cfdb-056-enrich-port-split, and why the port cannot be `GraphView`.** `GraphView` (`cfdb-core/src/graph.rs:31-65`) is *id-string-keyed and write-oriented*: it was shaped for passes that walk a label once and set attributes. The evaluator is *handle-keyed and read-oriented*: it needs stable `Copy`+`Ord` node/edge handles that flow through the binding table and sort deterministically, edge handles for `r.label`/`r.src`/`r.dst`, index posting-list acceleration (`candidates_from_index`, cfdb-035-persistent-inverted-indexes), and label/edge-label existence probes — none of which `GraphView` expresses (`nodes_with_label` returns `Vec<String>`, cloning every id on every MATCH; `neighbors` allocates a `String` per hop). Reusing `GraphView` for the evaluator would turn the hot path of every `cfdb query`/`violations`/`impact` invocation into the allocation class cfdb-056-enrich-port-split#6 accepted only once, for the reachability BFS, with a measurement. So this RFC adds a **read-only, handle-based sibling port**; it does not widen `GraphView`.

## 2. Scope

Ships:

### 2.1 `GraphReader`

**`GraphReader`** — new read-only trait in `cfdb_core::graph`, sibling of `GraphView` (§3.1). Named for what it is in the existing Graph-headed family (`GraphView` = read+write id-based, `GraphBackend` = factory, `GraphReader` = read-only handle-based; ddd R1 — the draft's `QueryGraph` collided with `cfdb_core::query::Query`, `ast.rs:29`, and inverted the family's compound-noun order). Handle-based: `NodeHandle` / `EdgeHandle` (`Copy + Ord + Hash + Debug`, `u32`-backed opaque newtypes, §3.3) replace `NodeIndex`/`EdgeIndex` in every evaluator signature and in `Binding`. Every method is `&self` — the "read-only (G2)" guarantee `StoreBackend::execute` states in prose today becomes a compiler-enforced property of the port the evaluator is coded against; the name says so.

### 2.2 `GraphBackend::graph_reader`

**`GraphBackend::graph_reader(&self, &Keyspace) -> Result<&dyn GraphReader, StoreError>`** — the read-only sibling of the existing `graph_view` (`graph.rs:76`), on the existing factory trait. No new factory trait (reuse, not a fourth `*Backend`): resolving a keyspace is one concept whether the caller wants to mutate or read. `GraphBackend` has exactly one implementor workspace-wide (`graph_view_backend.rs:83`), so a new required method breaks nothing else (clean-arch R1).

### 2.3 `QueryBackend`

**`QueryBackend`** — new trait in `cfdb_core::store`, carrying exactly the `execute(&self, &Keyspace, &Query) -> Result<QueryResult, StoreError>` method today declared on `StoreBackend` (`store.rs:71-72`), moved verbatim. `StoreBackend` loses `execute` and becomes a pure storage port (ingest / schema_version / list / drop / canonical_dump; `cfdb-core/tests/signatures.rs` + `signatures.toml` freeze "all seven" methods today — becomes six). ISP: `cfdb-cli/src/scope/helpers.rs:29,76` already takes `&dyn StoreBackend` *only* to call `execute` ("depends only on the port contract (`execute`)", `helpers.rs:26`; solid R1 grepped every `StoreBackend` method's call sites — these two are the only `&dyn StoreBackend` consumers and both call nothing else) — after this RFC it takes `&dyn QueryBackend`, the narrower contract it actually meant. `QueryBackend`/`QueryEngine` keep the capability-prefix grammar of `StoreBackend`/`EnrichBackend`/`EnrichEngine` (ddd R1: "Query" as modifier is correct there; only the Graph-headed trait needed to drop it).

### 2.4 The `cfdb-eval` crate

**`cfdb-eval`** — new workspace crate (root `Cargo.toml` `[workspace] members` gains `crates/cfdb-eval` — load-bearing, named explicitly per rust-systems R1). Production deps: `cfdb-core` + `regex` (the `regexp_extract`/`=~` UDF cache, `eval/mod.rs:120`; `regex.workspace = true` carries no per-crate feature overrides anywhere, so the two crates cannot drift) only. `[dev-dependencies]`: `cfdb-petgraph` (the only concrete `GraphBackend`, exactly cfdb-056-enrich-port-split#2's precedent and rationale — real infra over a hand-rolled fake graph) + `cfdb-query` (the moved test suites parse Cypher text, as `cfdb-petgraph`'s own `[dev-dependencies] cfdb-query` does today). Holds the moved `eval/` tree rewritten against `GraphReader`, the moved `explain.rs` (`ExplainRow`/`ExplainHit`, 71 lines, produced exclusively by `Evaluator::record_explain`, `eval/mod.rs:242-247` — evaluator observability, not storage observability; ddd R1 confirmed owner), and `QueryEngine`. `cfdb-eval/src/lib.rs` carries the same `#![allow(unknown_lints)] #![deny(non_exhaustive_omitted_patterns)]` pair as `cfdb-petgraph/src/lib.rs:23-24` — the cross-crate matches on `#[non_exhaustive]` `RowValue`/`WarningKind` move with the code, so does their forward-compat guard (rust-systems R1). Carries no Cargo features.

### 2.5 `QueryEngine`

**`QueryEngine<'s, S: GraphBackend>`** in `cfdb-eval` (§3.2), implementing `cfdb_core::store::QueryBackend` generically, plus the inherent `execute_explained` (cfdb-035-persistent-inverted-indexes#4 kept explain off `StoreBackend`; it stays off `QueryBackend` for the same reason — it is an evaluator diagnostic, and it moves with the evaluator). Both reproduce `lib.rs:182-198,222-231` verbatim: keyspace guard (inside `graph_reader`), evaluate, **prepend ingest warnings** via a port method (§3.1) — byte-identical `QueryResult.warnings`.

### 2.6 `cfdb-cli` gains the dependency

**`cfdb-cli` gains `[dependencies] cfdb-eval`** (clean-arch + solid R1: the composition root constructs `QueryEngine`, so `cfdb-cli` is a real production consumer of the new crate — the draft omitted the edge). `.cfdb/workspace-dep-rules.toml` has no `[cfdb-cli]` section by existing convention (the entry point composes every adapter; it is exempt from the CLEAN-3 gate) — this RFC leaves that convention as-is and states it rather than silently relying on it. Optional, non-blocking (clean-arch R1): a one-line `compose::query_engine(&store) -> QueryEngine<'_, PetgraphStore>` helper for discoverability, consistent with `compose.rs`'s own charter — the ten `src/` sites have no central dispatcher to hook into, unlike cfdb-056-enrich-port-split's `enrich.rs`.

### 2.7 `cfdb-petgraph` implements the port

`cfdb-petgraph` implements `GraphReader` on `KeyspaceState` (thin delegation to the existing `pub(crate)` methods `nodes_with_label`/`all_nodes_sorted`/`has_label`/`has_edge_label` (`graph.rs:297-322`), `materialized_ingest_warnings` (`graph.rs:202`), and `index::lookup::candidates_from_index` (`index/lookup.rs:150-192`)); `index/` (spec, build, lookup) **stays in `cfdb-petgraph`** as the store's index subsystem (§3.1 rationale, §6 — a stated trade against measured CCP evidence, not a CCP-neutral choice).

### 2.8 Composition-root cutover

**Composition-root cutover is per-slice, same-PR, no transient reverse edge** (cfdb-056-enrich-port-split#2's ratified rule, applied to the one verb this RFC moves): the slice that moves `eval/` out of `cfdb-petgraph` (057-B) also (a) deletes `StoreBackend::execute` from the trait and `PetgraphStore::execute_explained`, (b) flips **all thirteen** `store.execute(...)` call sites in `cfdb-cli` to `QueryEngine` — the ten in `src/` (`check_predicate.rs:120`, `stubs.rs:72`, `scope/explain_sink.rs:54,60`, `commands/query.rs:58,134`, `commands/rules.rs:97`, `commands/impact.rs:67,119`, `scope/helpers.rs:60,119`) **and the three in `tests/`** (`impact_hir_dogfood.rs:88`, `impact_seed_binding.rs:111`, `scope_classifier_perf.rs:346`, each importing `cfdb_core::store::StoreBackend` and constructing a `PetgraphStore` directly — clean-arch + solid R1: the draft's grep excluded `tests/` and would have left 057-B not compiling), plus `explain_sink.rs:17`'s `use cfdb_petgraph::explain::ExplainRow` → `cfdb_eval::explain::ExplainRow` (ddd R1), and (c) moves every `cfdb-petgraph` test that exercises `execute` (`src/tests.rs`'s query-shaped tests, `src/eval/*_tests.rs`, `tests/{hsb_cluster,pattern_b_vertical_split_brain,pattern_b_vertical_split_brain_drop,raid_plan_queries,vsb_multi_resolver,cartesian_candidate_cache}.rs`) into `cfdb-eval`, so `cfdb-petgraph` never depends on `cfdb-eval` — not in `[dependencies]`, not in `[dev-dependencies]`. **Why the dev direction is closed too** (rust-systems R1 corrected the draft's rationale — a purely dev-dev mutual edge is legal Cargo and does not duplicate crates; the trap is a *normal* edge one way plus a dev edge the other, which is exactly the rejected alternative's shape, §6): the edge is simply unnecessary once the tests move, and keeping the relationship one-directional is what cfdb-056-enrich-port-split's `cfdb-enrich`↔`cfdb-petgraph` precedent already does. Prescribed, not merely recommended (§4).

### 2.9 Dep rules

`.cfdb/workspace-dep-rules.toml` gains `[cfdb-eval]` (allowed: `cfdb-core`, `regex`; forbidden: `cfdb-petgraph`, `cfdb-query`, `cfdb-enrich`, `cfdb-extractor*`, `cfdb-recall`, `cfdb-cli`, `petgraph`, `indexmap`, `toml`, `serde_json`, and the existing service-crate list) plus a sixth `tests/architecture_dep_rule.rs` sibling in `cfdb-eval` (the other six gated crates each carry one — verified). `[cfdb-core]`, `[cfdb-petgraph]`, `[cfdb-enrich]`, `[cfdb-query]` `.forbidden` gain `cfdb-eval` (tripwires, same non-cycle rationale as cfdb-056-enrich-port-split#2). `[cfdb-petgraph].allowed` keeps `regex` — its comment (`workspace-dep-rules.toml:99`, "predicate evaluator (regexp_extract)") goes stale and is rewritten to the real remaining reason, `index/spec.rs`'s `ComputedKey::ConversionPrefix` (`CONVERSION_PREFIX_PATTERN`, `spec.rs:181`) (solid + rust-systems R1). The dev half of the no-edge rule gets a machine check too (clean-arch + rust-systems R1, converged): **not** by generalising `architecture_dep_rule.rs`'s scanner (today `[dependencies]`-only, `cfdb-petgraph/tests/architecture_dep_rule.rs:76-89`) to `[dev-dependencies]` — that would misfire on `cfdb-eval`'s own legitimate dev edge to `cfdb-petgraph`, which the same file lists under `.forbidden` because "forbidden" is deliberately dependencies-only — but by one crate-specific assertion added to `cfdb-petgraph/tests/architecture_dep_rule.rs`: `cfdb-eval` appears nowhere in that crate's `Cargo.toml`, either section. That is the one edge with zero legitimate exception. Rides in 057-B (§7).

### 2.10 Cross-crate invariant pin

**Cross-crate invariant pin** (solid R1, blocking): `index/spec.rs:172-190` documents that `CONVERSION_PREFIX_PATTERN` (`^(\w+)_(?:from|to|for|as)_`) MUST equal the `regexp_extract` literal the evaluator's `call_regexp_extract` (`eval/predicate.rs:139`) is fed by `examples/queries/classifier-random-scattering.cypher:74-75`; today nothing asserts it — same-crate proximity was the only guard, and 057-B puts a no-edge wall between const and UDF. 057-B ships the pin: `CONVERSION_PREFIX_PATTERN` widens `pub(crate)` → `pub`, and a `cfdb-eval` test (through its `[dev-dependencies] cfdb-petgraph` edge — the seam already permitted) asserts the const byte-equals the pattern literal in that `.cypher` file. §7.

### 2.11 The spec files

`specs/concepts/cfdb-eval.md` (new — sections `QueryEngine`, `ExplainRow`, `ExplainHit`; `Evaluator` stays `pub(crate)` like `EnrichEngine`'s pass internals — ddd R1 confirmed complete), `specs/concepts/cfdb-core.md` (`GraphReader` slots between `GraphBackend`/`GraphView` alphabetically; `QueryBackend`; `NodeHandle`/`EdgeHandle`), `specs/concepts/cfdb-petgraph.md` (`execute_explained` removed; **`## ExplainRow` / `## ExplainHit` sections removed** — they move to `cfdb-eval.md`, `cfdb-petgraph.md:33-39` today; `GraphReader` impl noted). `.gitea/workflows/ci.yml` needs **no** new step (rust-systems R1: there is no per-crate `cfdb-petgraph` step today; `cfdb-enrich`'s exists only for its feature-OFF paths; a feature-less workspace member is covered by the existing `--workspace --all-features` clippy/nextest steps, `ci.yml:150,158`).

### 2.12 Does not ship

Does **not** ship (§6): any change to Cypher grammar or evaluator semantics; any change to `GraphView`; any move of `index/` out of `cfdb-petgraph`; any `SchemaVersion` bump; any wire-format change; a second `GraphReader` implementor. Acceptance bar for every slice: existing suite passes with zero assertion changes **and** self-dogfood query output through the real `cfdb` binary is byte-identical to the pre-slice baseline (§4), plus the named perf measurement in 057-A (§3.4).

## 3. Design

### 3.1 The port

#### 3.1.1 `NodeHandle`

```rust
// cfdb-core::graph — read-only, handle-based sibling of GraphView.

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeHandle(u32);          // opaque; see §3.3
```

#### 3.1.2 `EdgeHandle`

```rust

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EdgeHandle(u32);
impl NodeHandle { pub const fn from_raw(u32) -> Self; pub const fn raw(self) -> u32; }   // ditto EdgeHandle

```

#### 3.1.3 `GraphReader`

```rust
pub trait GraphReader {
    // existence + vocabulary (pattern.rs:147,150; path.rs:41,46)
    fn has_label(&self, label: &Label) -> bool;
    fn labels(&self) -> Vec<Label>;                     // by_label.keys(), sorted (BTreeMap order)
    fn has_edge_label(&self, label: &EdgeLabel) -> bool;
    fn edge_labels(&self) -> Vec<EdgeLabel>;            // BTreeSet order

    // scans (pattern.rs:174,177) — same order the store already returns today
    fn nodes_with_label(&self, label: &Label) -> Vec<NodeHandle>;
    fn all_nodes_sorted(&self) -> Vec<NodeHandle>;

    // dereference (pattern.rs:194,199; path.rs:145; predicate.rs:63,65,85,86)
    fn node(&self, h: NodeHandle) -> Option<&Node>;
    fn edge(&self, h: EdgeHandle) -> Option<&Edge>;

    // adjacency (path.rs:251-275) — (edge, other endpoint), one call per direction,
    // Vec because collect_directed_edges already materialises a Vec today
    fn edges_out(&self, h: NodeHandle) -> Vec<(EdgeHandle, NodeHandle)>;
    fn edges_in(&self, h: NodeHandle) -> Vec<(EdgeHandle, NodeHandle)>;

    // RFC-035 index fast path (pattern.rs:163-169 → index/lookup.rs:150-192)
    // None = no usable hint → caller falls back to a scan;
    // Some(vec![]) = provably empty (lookup.rs:181) — a result, NOT a fallback.
    fn index_candidates(
        &self,
        np: &NodePattern,
        where_clause: Option<&Predicate>,
        params: &BTreeMap<String, ParamBinding>,
        bound_var_prop: &dyn Fn(&str, &str) -> Option<PropValue>,   // (var, prop) → value of an already-bound var
    ) -> Option<Vec<NodeHandle>>;

    // coupling.rs:161-166 — is (label, tag) both declared indexed and non-empty?
    fn indexed_prop_is_populated(&self, label: &Label, tag: &str) -> bool;

    // lib.rs:194-196,229-230 — RFC-054 §3.4 ingest diagnostics the engine prepends
    fn ingest_warnings(&self) -> Vec<Warning>;
}

// existing trait, one added method (read-only sibling of graph_view)
```

#### 3.1.4 `GraphBackend::graph_reader`

```rust
pub trait GraphBackend: Send + Sync {
    fn graph_view(&mut self, keyspace: &Keyspace) -> Result<&mut dyn GraphView, StoreError>;
    fn graph_reader(&self, keyspace: &Keyspace) -> Result<&dyn GraphReader, StoreError>;   // NEW
    fn workspace_root(&self) -> Option<&Path>;
}
```

#### 3.1.5 `QueryBackend`

```rust
// cfdb-core::store — execute leaves StoreBackend, verbatim, into its own contract.
pub trait QueryBackend: Send + Sync {
    /// Evaluate a parsed Query against the given keyspace. Read-only (G2).
    fn execute(&self, keyspace: &Keyspace, query: &Query) -> Result<QueryResult, StoreError>;
}
```

#### 3.1.6 Why the port has this exact surface

Why the port has this exact surface and no more — every method maps to one cited reach-in in §1 (solid R1 diffed all 16 distinct `state.X` reach-ins under `eval/**` against this list: 1:1, no YAGNI addition, no missed reach-in); every type it names is a `cfdb-core` type today (`NodePattern`/`Predicate`/`ParamBinding` at `query/ast.rs:54,81,122`, `PropValue` at `fact.rs:20`, `Warning` at `result.rs:63` — clean-arch R1):

- `index_candidates` keeps **`index/lookup.rs` (hint collection, cross-MATCH computed-key equi-join, posting-list intersection) inside `cfdb-petgraph`**, exposed as one port method that takes the query-AST types it already takes today. The alternative — moving hint collection into `cfdb-eval` and exposing raw posting lists — would require moving `IndexSpec`/`IndexTag`/`IndexValue`/`ComputedKey` (`index/spec.rs`, 570 lines, cfdb-035-persistent-inverted-indexes's store-side config vocabulary) into `cfdb-core` and either returning borrowed `&BTreeSet<NodeHandle>` (forcing the store to key its posting lists by the port's handle type) or allocating a `Vec` per hint probe inside `intersect`'s `retain` loop. cfdb-035-persistent-inverted-indexes placed the index subsystem in the store; this RFC leaves it there (as a stated trade — §6 records the CCP evidence that cuts the other way). Today's concrete `candidates_from_index`/`collect_where_hints` take a generic `F: Fn(&str,&str) -> Option<IndexValue>` (`index/lookup.rs:150-158,227-237`); the port takes `&dyn Fn(&str,&str) -> Option<PropValue>` — `&dyn` because a generic `F` would break dyn-safety (clean-arch + rust-systems R1 agreed it is the only dyn-safe choice), and `PropValue` (a core type) instead of `IndexValue` (a store type) because the closure at `pattern.rs:185-196` does `props.get(prop)` then `crate::index::build::index_key_of(pv)` today — the port moves that store-internal conversion behind the boundary. **This is a fix, not just a rationale detail** (ddd R1): the evaluator currently calls a store-internal conversion function directly; after this RFC it cannot.
- `indexed_prop_is_populated` is the `coupling.rs:161-166` probe (`state.by_prop.get(&(label.clone(), tag.clone())).is_some_and(|bucket| !bucket.is_empty())`) — the measured cartesian-hoist guard. It stays a port method rather than being derived from `index_candidates` because the caller has no `NodePattern` at that point. `tag: &str` is not under-typed: `IndexTag` is `pub(crate) type IndexTag = String` (`index/build.rs:42`) with its own doc calling it provisional; a core newtype would over-type ahead of the store's own commitment (ddd R1 — revisit in lockstep if `IndexTag` ever becomes an enum).
- `node`/`edge` return `Option` (not panic) — today `state.graph[idx]` panics on a stale index; a handle can only come from this same reader in the same `Evaluator` run, so `None` is unreachable in practice, but the port must not promise more than it can check. The evaluator treats `None` exactly as it treats a missing prop today (binding→null); no new warning text.
- `edges_out`/`edges_in` return `Vec` — same allocation class as today (`collect_directed_edges` builds a `Vec<(NodeIndex, EdgeIndex)>` per hop already, `path.rs:257-258`); the BFS `visited: BTreeSet<NodeHandle>` / `queue: VecDeque<(NodeHandle, u32)>` (`path.rs:222-224`) stay `Copy`-keyed — **no String-keyed visited set**, the specific regression cfdb-056-enrich-port-split#6 accepted for 056-F is not paid here. Two methods rather than a `Direction` parameter keeps `Direction` inside query-evaluation vocabulary (`EdgePattern.direction`, dispatched by the evaluator at `path.rs:184-186,236-239`) instead of crossing the storage port as `GraphView::neighbors` lets it (ddd R1: an improvement over the cfdb-056-enrich-port-split precedent; `collect_directed_edges` already decomposes into two petgraph calls, so this is a 1:1 mapping, not a reimplementation).
- `labels()`/`edge_labels()` exist only for the unknown-label / unknown-edge-label warning text (`pattern.rs:150`, `path.rs:46`) — cold path.
- `ingest_warnings()` is what lets `QueryEngine::execute` reproduce `lib.rs:229-230` byte-for-byte; without it the cfdb-054-target-identity-namespace warning prepend would silently vanish from every query result. Single `Warning` type workspace-wide; name mirrors `PetgraphStore::ingest_warnings` / `KeyspaceState::materialized_ingest_warnings` (ddd R1).

`GraphReader` is dyn-safe (no generics, no associated types, no `Self` returns; `Option<&Node>` is elision-rule-3 on `&self`; rust-systems R1 compiled the exact shape) so `GraphBackend::graph_reader` can hand out `&dyn GraphReader` — mirroring `graph_view`, and coexisting with it on one trait (different receiver mutability, no elision conflict). `Evaluator` is written as `Evaluator<'a, G: GraphReader + ?Sized>` holding `state: &'a G` (rust-systems R1 compiled it over `G = dyn GraphReader` including `BindingStream<'e>` and the re-entrant `NOT EXISTS` construction): it compiles against `dyn GraphReader` (the engine path) *and* against a concrete `KeyspaceState` (in-crate tests at 057-A, and the monomorphised escape hatch if 057-A's measurement (§3.4) shows the vtable indirection is not noise — in which case `GraphBackend` gains an associated `type Reader: GraphReader` in a follow-up, not silently here).

### 3.2 `QueryEngine`

```rust
// cfdb-eval::engine
pub struct QueryEngine<'s, S> { store: &'s S }

impl<'s, S: GraphBackend> QueryEngine<'s, S> {
    pub fn new(store: &'s S) -> Self { Self { store } }

    /// lib.rs:182-198 moved verbatim: guard (inside graph_reader), evaluate with explain, prepend ingest warnings.
    pub fn execute_explained(&self, keyspace: &Keyspace, query: &Query)
        -> Result<(QueryResult, Vec<ExplainRow>), StoreError>
    {
        let g = self.store.graph_reader(keyspace)?;
        let (mut result, explain) = Evaluator::new_with_explain(g, &query.params).run_explained(query);
        let mut prepended = g.ingest_warnings();
        prepended.append(&mut result.warnings);
        result.warnings = prepended;
        Ok((result, explain))
    }
}

impl<'s, S: GraphBackend> QueryBackend for QueryEngine<'s, S> {
    fn execute(&self, keyspace: &Keyspace, query: &Query) -> Result<QueryResult, StoreError> {
        let g = self.store.graph_reader(keyspace)?;                     // guard #1 lives here
        let mut result = Evaluator::new(g, &query.params).run(query);
        let mut prepended = g.ingest_warnings();                          // RFC-054 §3.4, lib.rs:229-230
        prepended.append(&mut result.warnings);
        result.warnings = prepended;
        Ok(result)
    }
}
```

`QueryEngine` holds `&'s S` (shared), not `&'s mut S` as `EnrichEngine` does — evaluation is read-only and the CLI already evaluates through `&PetgraphStore`. `QueryBackend: Send + Sync` is satisfied for `S: GraphBackend` because `GraphBackend: Send + Sync` already (cfdb-056-enrich-port-split#3.1) and `&S: Send` needs only `S: Sync` (rust-systems R1 compiled the `assert_send_sync` for both a concrete store and `dyn GraphBackend`). `S` is **not** `?Sized` (rust-systems R1 trim, same posture as cfdb-056-enrich-port-split#3.1's GAT rejection): no call site builds a `QueryEngine<dyn GraphBackend>` — `compose.rs` returns concrete `PetgraphStore` at every site, and `helpers.rs`'s `&dyn QueryBackend` unsizes the *engine* (a different trait object), which needs nothing from `S`. `QueryEngine` is pure dispatch + the two moved lines; no evaluator logic grows into it (cfdb-056-enrich-port-split#3.2's boundary rule restated).

The composition root (`compose.rs`) keeps returning a concrete `PetgraphStore` by value exactly as today ("the concrete return keeps the adapter seam honest", `compose.rs:14-19`) and each of the thirteen `cfdb-cli` sites wraps it in `QueryEngine::new(&store)` — the same one-line flip cfdb-056-enrich-port-split made per verb in `cfdb-cli/src/enrich.rs`. Per-site construction is not a composition-root violation: no adapter *selection* happens there (clean-arch R1); the optional `compose::query_engine` helper (§2) is a DRY nicety, not a correctness requirement.

`cfdb-cli/src/scope/explain_sink.rs::run` (`:47-62`) takes `&PetgraphStore` today solely to reach the inherent `execute_explained`; it takes `&QueryEngine<'_, PetgraphStore>` after 057-B — same shape, one type change.

### 3.3 Handles and determinism

`NodeHandle(u32)` / `EdgeHandle(u32)` wrap exactly `NodeIndex::index() as u32` / `EdgeIndex::index() as u32` (`KeyspaceState.graph` is `StableDiGraph<Node, Edge>` with petgraph's default `Ix = u32`, `graph.rs:30` — the cast is lossless, rust-systems R1). `Ord` on the handle is derived over the plain `u32` populated from `.index()`, and every existing tie-break already sorts on `.index()` — so `path.rs:190`'s `sort_by_key(|(n, e)| (*n, e.map(|i| i.index())))` / `:247`'s `sort_by_key(|(n, _)| *n)`, `pattern.rs`'s `BTreeSet<NodeIndex>` collections, and every "sorted by index" tie-break keep the *identical* ordering — definitionally the same total order, not "probably matches" (rust-systems R1). G1 preserved by construction: the store's `nodes_with_label` (`graph.rs:297-302`, `BTreeSet<NodeIndex>` iteration) and `all_nodes_sorted` (`:305-310`) are wrapped, not re-derived. `from_raw`/`raw` are `pub` and are **the only legal construction path**: `impl From<NodeIndex> for NodeHandle` in `cfdb-petgraph` violates the orphan rule (trait, `Self`, and the type parameter are all foreign to that crate — rust-systems R1); the evaluator never *interprets* a raw value beyond `Ord`/`Hash` — the tripwire test in 057-A (§7) greps non-test `eval/` sources for `.raw()` and `from_raw(` and permits neither.

Why not reuse `NodeIndex` in the port (rust-systems pre-trim): it would put `petgraph` in `cfdb-core`'s dependency set — the hub crate that today has `Ce = 0` toward any workspace crate (`[dependencies]` = serde/serde_json/thiserror only, solid R1) — and would let a second backend leak its own index type through a public port. Two `u32` newtypes cost nothing and keep the hub engine-free.

### 3.4 Migration order (strangler-fig; seam first, crate boundary second)

Unlike cfdb-056-enrich-port-split's 7 independent verbs, the evaluator is one pipeline: there is no verb-by-verb cutover. The strangling is by *seam*, then by *crate*:

| Slice | Change | Composition-root edit (same PR) | Why this slot / acceptance |
|---|---|---|---|
| 057-0 | `NodeHandle`/`EdgeHandle` + `GraphReader` + `GraphBackend::graph_reader` + `QueryBackend` in `cfdb-core`; `impl GraphReader for KeyspaceState` in `cfdb-petgraph` (thin delegation, next to the existing `impl GraphView`); baseline capture (§7) | none — `cfdb-cli` still calls `PetgraphStore::execute` | Additive only. Proves the port compiles and delegates identically before anything depends on it. `QueryBackend` is declared but has no implementor yet (like `GraphBackend`'s trait-only slice 056-0). |
| 057-A | **In-place** rewrite of `eval/` *inside* `cfdb-petgraph` against `&dyn GraphReader`: `Binding::{NodeRef(NodeHandle), EdgeRef(EdgeHandle)}`, `Evaluator<'a, G: GraphReader + ?Sized>`, `coupling.rs`'s three `&KeyspaceState` params → `&G`, `candidates_from_index` call + `bound_var_index_value`'s `index_key_of` use → `g.index_candidates(..)`, `collect_directed_edges` → `edges_out`/`edges_in`; `explain.rs` untouched (still in-crate). Tripwire test on **non-test** sources under `src/eval/` (`*_tests.rs` and `#[cfg(test)]` fixture files legitimately keep constructing `KeyspaceState` to instantiate `Evaluator<'_, KeyspaceState>` — rust-systems R1: `cross_match_tests.rs:34,111`, `fast_path_tests.rs:32,63,64,106`, `target_dogfood_tests.rs:32,197`; only `fast_path_tests.rs:29`'s `petgraph::stable_graph::NodeIndex` import goes away, its `BTreeSet<NodeIndex>` becoming `BTreeSet<NodeHandle>`): no `petgraph::`, no `crate::graph::KeyspaceState`, no `crate::index::`, no `.raw()`, no `from_raw(` (grep-shaped, same family as `tests/architecture_dep_rule.rs`); `lib.rs:222-231` becomes `Evaluator::new(state as &dyn GraphReader, ..)` | none | The seam is proven **inside** the crate before it is crossed — the diff is pure evaluator, no `Cargo.toml`, no CLI, no test relocation, so a byte-identity failure here is unambiguously the rewrite. **This is the slice that carries the perf measurement**: same binary, only the seam changed → clean A/B, uncontaminated by the compile-unit change 057-B brings (solid R1: this is why A and B stay separate, not taste). Named measurement (§7): wall-clock on the impact/var-length path (`cfdb impact` on the persisted `.cfdb/db/cfdb-self-378.json` HIR keyspace — the 246k-`CALLS` BFS cfdb-056-enrich-port-split#3.4 named), plus the classifier-shaped cartesian rules `.cfdb/queries/{hsb-cluster,vsb-multi-resolver}.cypher`, pre (develop) vs post, recorded in the PR body. Expected class: vtable call per hop on top of the `Vec` per hop already paid — noise (rust-systems R1 concurs); if not noise, the associated-type escape hatch (§3.1) is filed as a follow-up before 057-B, not folded silently. |
| 057-B | Crate crossing: new `cfdb-eval` (`Cargo.toml`, `lib.rs` with the lint pair, workspace member), `git mv` `eval/` + `explain.rs` + every `execute`-exercising `cfdb-petgraph` test (§2 list) into it; `QueryEngine` (§3.2) implementing `QueryBackend`; **delete `StoreBackend::execute` from the trait, delete `PetgraphStore::execute_explained` and `lib.rs`'s `use crate::eval::Evaluator`**; `cfdb-cli/Cargo.toml` `+ cfdb-eval`; flip the **thirteen** `cfdb-cli` sites (ten `src/`, three `tests/`) + `helpers.rs`'s two `&dyn StoreBackend` params → `&dyn QueryBackend` + `explain_sink.rs::run`'s param + `explain_sink.rs:17`'s import; `cfdb-petgraph/Cargo.toml` — `regex` stays (index), nothing else changes; the `CONVERSION_PREFIX_PATTERN` cross-crate pin (§2); dep-rules `[cfdb-eval]` + tripwires + `cfdb-eval/tests/architecture_dep_rule.rs` + the `regex` comment rewrite; `specs/concepts/{cfdb-eval,cfdb-core,cfdb-petgraph}.md`; `cfdb-core/tests/signatures.rs` + `signatures.toml` (seven → six) | all thirteen `cfdb-cli` sites, same PR | Mechanical after 057-A (the code already speaks only `GraphReader`; this slice moves files and re-points call sites). Same-PR trait-method removal + call-site flip is what keeps `cfdb-petgraph` free of any `cfdb-eval` edge at every commit on `develop` (§2, §4). Cannot be split further without either a duplicate live evaluator (violates the "exactly one live evaluator" invariant below) or a reverse edge (solid + clean-arch R1 converged: forced, not style). Acceptance: moved suites pass with zero assertion changes; self-dogfood diff-empty against the 057-0 baseline; `cargo tree -p cfdb-petgraph -e normal,dev` shows no `cfdb-eval`; `cargo tree -p cfdb-eval -e normal` shows no `petgraph`. |
| 057-C | Cutover cleanup: `cfdb-petgraph/src/lib.rs` module doc (`:7-10` "Evaluation is routed through `eval::Evaluator`") + `specs/concepts/cfdb-petgraph.md` prose; `cfdb-core/src/store.rs:1-10` module doc ("consumers then call `backend.execute(&query)`") → names `QueryBackend`; `docs/query-dsl.md:23` / `docs/udfs.md:4,9,227` paths to `crates/cfdb-petgraph/src/eval/predicate.rs` → `crates/cfdb-eval/…`; `cfdb-petgraph`'s `[dev-dependencies] cfdb-query` — dropped **iff** no remaining test in the crate parses Cypher after 057-B's relocation (verify by grep at the time, not assumed); `.cfdb/queries/*.cypher` comment paths | none | Pure doc/dep hygiene, zero code motion. **Default: fold into 057-B** (solid R1 lean — low marginal review value alone); kept as a named slice so that if 057-B's reviewer wants the move-and-re-point diff kept pure, the cleanup has a home. Either resolution is fine. |

Each slice's acceptance gate is the existing suite with **zero assertion changes** plus the self-dogfood diff (§4). No slice leaves `develop` in a state where `cfdb query` on the shipped binary evaluates through anything but exactly one live evaluator.

## 4. Invariants

- **No wire-format change. No `SchemaVersion` bump. No graph-specs lockstep.** `Node`/`Edge`/`QueryResult`/`Warning` shapes untouched. `graph-specs-rust` and `qbot-core` consume cfdb through the `cfdb` binary (`graph-specs-rust` has no Cargo edge on any cfdb crate — verified: `.cfdb/cfdb.rev` + `ci/*.sh` only), so `StoreBackend` losing a method is a workspace-internal API change, not a downstream one.
- **Determinism (G1) preserved by construction**: every ordered read (`nodes_with_label`, `all_nodes_sorted`, `labels`, `edge_labels`, `edges_out`/`edges_in`) is a wrapper over the store's existing ordered accessor; handle `Ord` ≡ raw index `Ord` (§3.3). A slice that changes iteration order or a tie-break is a bug, not a refactor.
- **Read-only (G2) becomes compiler-enforced**: `GraphReader` has no `&mut self` method; `QueryEngine` holds `&S`; the evaluator cannot mutate a keyspace by construction rather than by trait-doc promise.
- **Behavior-identical, not merely "tests pass"**: for every `.cypher` under `.cfdb/queries/` and `examples/queries/`, plus the `cfdb impact` and `cfdb scope --explain` paths, `cfdb query --db … --keyspace … --format json` output (rows **and** `warnings`, incl. the cfdb-054-target-identity-namespace ingest-warning prepend order) through the real binary on the same extract of cfdb's own tree is byte-identical to the 057-0 baseline at every slice. Recall: N/A (no extractor change).
- **No `cfdb-eval` edge into `cfdb-petgraph`, normal or dev.** Not because Cargo forbids a dev-dev pair (it does not — rust-systems R1), but because the direction is one-way by design (cfdb-056-enrich-port-split precedent) and the tests that would want it move out in 057-B. Enforced: dep-rules `[cfdb-petgraph].forbidden += cfdb-eval` (the existing `architecture_dep_rule.rs` gate, `[dependencies]`), the crate-specific "`cfdb-eval` appears nowhere in `cfdb-petgraph/Cargo.toml`" assertion (§2, both sections), the 057-B PR-body `cargo tree -e normal,dev` proof, and the test relocation itself.
- **Cross-crate literal invariant is tested, not commented**: `CONVERSION_PREFIX_PATTERN` ≡ the `classifier-random-scattering.cypher` `regexp_extract` literal, pinned in `cfdb-eval` from 057-B on (§2).
- **No-ratchet**: no baseline/threshold file. 057-A's perf numbers live in the PR body as evidence.

## 5. Architect lenses

### 5.1 Clean architecture (`clean-arch`) — R1: REQUEST CHANGES → folds applied → R2: RATIFY

### 5.2 Domain-driven design (`ddd-specialist`) — R1: REQUEST CHANGES → folds applied → R2: RATIFY

### 5.3 SOLID / component principles (`solid-architect`) — R1: REQUEST CHANGES → folds applied → R2: RATIFY

### 5.4 Rust systems (`rust-systems`) — R1: REQUEST CHANGES → folds applied → R2: RATIFY

## 6. Non-goals

- **Moving `index/` (spec/build/lookup) out of `cfdb-petgraph`.** cfdb-035-persistent-inverted-indexes placed the inverted-index subsystem in the store; this RFC exposes it through one port method (§3.1) and does not relocate `IndexSpec`/`IndexValue`/`ComputedKey`. **This is a trade, and the CCP evidence cuts the other way for half of it** (solid R1): `index/lookup.rs` co-changes with `eval/` in 6 of its 14 commits and with storage in 1, while `index/spec.rs`+`build.rs` co-change with storage in 6 of 8. The seam inside `lookup.rs` is real and locatable (ddd R1): `collect_where_hints`/`resolve_eq_hint`/`resolve_cross_ref_computed_hint`/`unwrap_computed_call`/`match_computed_call` speak pure query-AST vocabulary (`Predicate`/`Expr`/`NodePattern`) and never touch `by_prop`; only `intersect`/`lookup_posting` touch the posting-list `BTreeMap`/`BTreeSet`. Four lenses converged that leaving it whole in the store is nonetheless right today, for compounding reasons: SDP/Dependency Rule (clean-arch — moving `IndexValue`/`IndexTag`/`ComputedKey`, the store's posting-list representation, into `cfdb-core` breaks the same `Ce = 0` hub invariant §3.3 protects for the handles, and a production `cfdb-eval` → `cfdb-petgraph` edge inverts the RFC), allocation cost of a raw-posting-list port (rust-systems, §3.1), no second implementor to force the translation (YAGNI, same posture as the GAT deferral), with solid's CCP numbers as the corroborating measurement of *when* the coupling would resolve. **Trigger condition**: revisit when a second `GraphReader` implementor exists — "hint recognition is query planning, posting lists are storage" is that RFC's split, cut exactly at the seam named above.
- **Rejected alternative — keep `StoreBackend::execute` and have `cfdb-petgraph` depend on `cfdb-eval`.** Smaller blast radius (zero CLI change, zero trait change), and the edge is *inward* (adapter → application service), so it is not a Dependency-Rule violation (clean-arch R1 independently confirmed). Rejected because it leaves the storage port carrying an application-service method (the exact ISP smell cfdb-031-audit-cleanup fixed for enrichment), leaves `PetgraphStore` as "the thing you call `execute` on" (the crate's public contract still spans storage + query, so the strangler-fig ends one step short of "pure storage adapter"), and puts the evaluator's tests in a worse place: either they stay in the store crate, or `cfdb-eval` takes a `[dev-dependencies] cfdb-petgraph` edge on top of `cfdb-petgraph`'s *normal* edge on `cfdb-eval`. rust-systems R1 compiled that asymmetric shape: Cargo accepts it — but `cfdb-eval`'s own unit-test binary then links `cfdb-eval` twice (once `cfg(test)`, once as the library `cfdb-petgraph` was built against), so any `cfdb-eval` type crossing that boundary does not unify, and promoting the dev edge to a normal one (an innocent non-test helper) is a hard `error: cyclic package dependency` one edit away (confirmed by compile). ISP plus that fragility, not a Cargo prohibition, is why P wins (clean-arch R1, re-grounded after the correction). The chosen shape has no cycle of any kind and lets the tests move with the code exactly as cfdb-056-enrich-port-split's did.
- **Widening `GraphView` to handles / migrating enrichment passes off `String` ids.** cfdb-056-enrich-port-split#6 deferred port perf; a `GraphReader`-shaped read surface would let 056-F's BFS drop its String-keyed visited set — a real, later perf RFC, not this one (this RFC ships zero behavior change to `cfdb-enrich`).
- **Associated-type (`type Reader: GraphReader`) monomorphisation on `GraphBackend`.** Not needed on the evidence; named as the escape hatch 057-A's measurement can trigger (§3.1, §3.4). Same posture cfdb-056-enrich-port-split#3.1 took on GATs.
- **Correctness re-evaluation of the evaluator** — `DEFAULT_VAR_LENGTH_MAX` semantics, `count()`-over-empty (#564), `NOT EXISTS` outer bindings (#546), the `eval_aggregation` sentinel-vs-`Result` deviation (#430) — all stay exactly as they are; Feathers discipline (characterize, strangle, *then* re-evaluate). #430's own text ("if Result cascade is chosen … the call chain … `apply_with` → … → `eval_aggregation`") is a `cfdb-eval` change after this RFC lands, and lands more cheaply then.
- **A `Box<dyn StoreBackend>` / `Box<dyn QueryBackend>` composition root.** `compose.rs` keeps returning concrete `PetgraphStore` (its own doc explains why, `compose.rs:14-19`); nothing here needs the boxed form.
- **Second backend, `serve --mcp` (#475), or any consumer-facing capability.** This RFC moves code across a crate boundary behind a port; it adds no verb, flag, fact type, or query construct.

## 7. Issue decomposition

Four slices (§3.4; 057-C defaults to folding into 057-B). Per CLAUDE.md §2.5's own classification 057-0 is new-capability-shaped (new port types), 057-A/B/C are mechanical-refactor-shaped ("the existing suite must pass byte-identically") — plus the self-dogfood diff restated every slice because it is the actual strangler-fig acceptance signal, run through the real `cfdb` binary. Slice issues cite `cfdb-057-eval-port-split#3.4` and carry these blocks verbatim.

**057-0 — Port + handles + `QueryBackend` + `impl GraphReader for KeyspaceState` (additive, zero eval change)**
```
Tests:
  - Unit: GraphReader delegation on KeyspaceState — for a synthetic fixture (a handful of labelled nodes,
    ≥1 multi-edge-label node, ≥1 indexed (label,tag) pair via IndexSpec, ≥1 recorded ingest warning):
    nodes_with_label/all_nodes_sorted/labels/edge_labels/has_label/has_edge_label/edges_out/edges_in/
    node/edge/index_candidates/indexed_prop_is_populated/ingest_warnings each equal the corresponding
    existing pub(crate) accessor or raw-field read, with handle.raw() == NodeIndex::index() as u32
    (this is the ONLY place .raw() is compared against a petgraph index — it lives in cfdb-petgraph).
    Compile-only pins: GraphBackend::graph_reader's elided lifetime compiles on PetgraphStore;
    QueryBackend: Send + Sync is object-safe (a fn taking &dyn QueryBackend compiles).
    Determinism: nodes_with_label(handles).map(raw) is strictly increasing on the fixture.
  - Self dogfood (cfdb on cfdb): capture the 057-0 baseline — cfdb extract --workspace . (syn) into a
    scratch db, then for every .cfdb/queries/*.cypher and examples/queries/*.cypher: cfdb query
    --format json (rows + warnings), plus cfdb impact on the persisted .cfdb/db/cfdb-self-378.json HIR
    keyspace and one cfdb scope --explain run; save as the diff target for 057-A/B/C.
  - Cross dogfood: none — no behavior change, nothing to compare on the companion.
  - Target dogfood: none — rationale: no evaluator code moved yet; nothing observable changed on qbot-core.
```

**057-A — In-place rewrite of `eval/` against `&dyn GraphReader` (inside `cfdb-petgraph`)**
```
Tests:
  - Unit: none new — mechanical refactor. Every existing eval test (src/eval/*_tests.rs, src/tests.rs
    query tests, tests/*.rs, index/lookup_*_tests.rs) passes with ZERO assertion changes.
    fast_path_tests.rs's "indexed lookup ≡ full scan + node_props_match post-filter" pin still
    reaches Evaluator::candidate_nodes / node_props_match (now generic over G, instantiated with the
    concrete KeyspaceState fixture) — the same pub(super) visibility, no widening; its BTreeSet<NodeIndex>
    becomes BTreeSet<NodeHandle> and its petgraph import goes.
    Tripwire (new, grep-shaped, same family as tests/architecture_dep_rule.rs), scoped to NON-TEST sources
    under src/eval/ (excludes *_tests.rs and #[cfg(test)] modules — those keep building KeyspaceState
    fixtures on purpose): no line matches `petgraph::` | `crate::graph::KeyspaceState` | `crate::index::` |
    `.raw()` | `from_raw(`. RED first: it fails on develop before the rewrite; GREEN after.
  - Self dogfood (cfdb on cfdb): every 057-0 baseline output byte-identical (rows AND warnings order).
    Perf evidence (recorded in the PR body, not a gate file): wall-clock, 3 runs each, pre (develop
    binary) vs post, for (a) `cfdb impact` on the cfdb-self-378 HIR keyspace (the 246k-CALLS var-length
    BFS), (b) `cfdb violations --rule .cfdb/queries/hsb-cluster.cypher` and vsb-multi-resolver.cypher
    on the same keyspace, (c) the full .cfdb/queries/*.cypher sweep on the syn extract. Report the
    delta; if it exceeds run-to-run noise, file the §3.1 associated-type follow-up BEFORE 057-B.
  - Cross dogfood (cfdb on graph-specs-rust at pinned SHA): ci/cross-dogfood.sh — 0 findings.
  - Target dogfood: none — rationale: same binary semantics, evaluator diff-empty on cfdb-self is the
    signal; the perf evidence above is the only additional measurement this slice owes.
```

**057-B — Crate crossing: `cfdb-eval`, `QueryEngine`, `StoreBackend::execute` removal, CLI cutover (+ 057-C by default)**
```
Tests:
  - Unit: none new for the move — mechanical refactor. Every moved suite passes unchanged in its new home
    (cfdb-eval/src/… + cfdb-eval/tests/…), running against S = PetgraphStore via [dev-dependencies].
    The three crates/cfdb-cli/tests/*.rs execute-callers (impact_hir_dogfood.rs:88,
    impact_seed_binding.rs:111, scope_classifier_perf.rs:346) flip to QueryEngine and pass unchanged.
    cfdb-core/tests/signatures.rs + signatures.toml updated for the removed StoreBackend::execute
    (seven → six; that test exists to make exactly this kind of change loud — update it in the same PR
    with a one-line rationale).
    NEW pin (solid R1): cfdb-eval test asserting cfdb_petgraph::index::spec::CONVERSION_PREFIX_PATTERN
    (widened to pub) byte-equals the regexp_extract pattern literal read from
    examples/queries/classifier-random-scattering.cypher:74-75 (parse the file, do not hardcode a third copy).
    Dep-rule gate: cfdb-eval/tests/architecture_dep_rule.rs (existing shape) for [cfdb-eval]: RED first on
    a deliberately-added cfdb-petgraph [dependencies] line, GREEN on the shipped Cargo.toml.
    Reverse-edge pin: cfdb-petgraph/tests/architecture_dep_rule.rs gains one assertion — the string
    `cfdb-eval` appears nowhere in crates/cfdb-petgraph/Cargo.toml (dependencies OR dev-dependencies).
    RED first on a deliberately-added [dev-dependencies] cfdb-eval line, GREEN on the shipped file.
  - Self dogfood (cfdb on cfdb): every 057-0 baseline output byte-identical THROUGH THE REAL BINARY
    (cfdb-cli's thirteen call sites now route through QueryEngine — this IS part of the slice).
    PR body carries: `cargo tree -p cfdb-petgraph -e normal,dev | grep -c cfdb-eval` == 0 and
    `cargo tree -p cfdb-eval -e normal | grep -c petgraph` == 0.
  - Cross dogfood (cfdb on graph-specs-rust at pinned SHA): ci/cross-dogfood.sh — 0 findings.
  - Target dogfood (qbot-core at pinned SHA): none — rationale: internal port refactor, no observable
    capability change; the self-dogfood byte-identity through the shipped binary is the load-bearing signal.
```

**057-C — Cutover cleanup (docs, dev-dep prune, spec prose) — only if not folded into 057-B**
```
Tests:
  - Unit: none — deletion/doc only. Existing suites pass byte-identically across all feature combos.
  - Self dogfood (cfdb on cfdb): 057-0 baseline byte-identical (regression check only).
  - Cross dogfood: ci/cross-dogfood.sh — 0 findings (regression check only).
  - Target dogfood: none — rationale: no code motion.
```

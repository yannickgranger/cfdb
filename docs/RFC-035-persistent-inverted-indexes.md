# RFC-035 — Persistent inverted indexes on `:Item` props and computed keys

Status: **draft, pre-council**
Parent trace: #167 → #168 (PR #177, merged) → #178 (query-time hash-join, narrowed) → **this RFC**
Companion: #167 trial evidence — 148k-node keyspace, 428 MB RSS (fixed), 16+ min scope wall time (unfixed).

---

## 1. Problem

Empirical trial on the #167 reproducer (qbot-core-trial-4055, 148 875 nodes / 149 361 edges) after #168 merged:

| Axis | Pre-#168 | Post-#168 | After RFC-035 (target) |
|---|---|---|---|
| Peak RSS | 13.13 GB (OOM-killed) | 428 MB | < 500 MB |
| `cfdb scope --context X` wall time | N/A (OOM) | > 16 min (killed) | < 60 s |

#168 fixed memory structurally (streaming evaluator, cartesian product no longer materialised). But **CPU is unchanged**: multi-MATCH queries still walk every pair. On `context_homonym`-shaped rules (21 432² ≈ 440 M candidate pairs for a result set of hundreds), scope verbs remain unusable.

#178 (query-time hash-join) improves this by O(n) per query but re-pays the hash-build cost on every invocation. cfdb keyspaces are **write-once / read-many**: extract once, query dozens of times via `scope` / `check` / `violations`. Paying the index cost once at extract time and amortising across every subsequent query is the architecturally correct move.

This RFC proposes **persistent inverted indexes** as part of the keyspace on-disk format.

---

## 2. Scope

### Deliverables

1. **Per-`(Label, prop)` inverted indexes** — `by_prop[(label, prop_name)][prop_value] → BTreeSet<NodeIndex>` built during `ingest_nodes`, stored on `KeyspaceState`, serialised as part of the JSON keyspace file.
2. **Computed-key registry** — a short allowlist of pure functions (`last_segment(qname)`, TBD others) that the extractor evaluates once and stores as virtual props. Indexes treat them like any other prop.
3. **Evaluator integration** — `candidate_nodes` consults `by_prop` when the MATCH pattern binds a prop literal; cross-MATCH Eq predicates intersect posting lists.
4. **Index-spec TOML** — per-keyspace `.cfdb/indexes.toml` naming which (label, prop) pairs are indexed. Adding an index is reviewed (per §6.8 no-ratchet rule).
5. **Lazy rebuild** — keyspaces serialised without the `indexes` block rebuild indexes on `persist::load`, emitting a one-line warning. Keeps the migration cost bounded.

### Non-deliverables

- **Cost-based query planner** — deferred. With persistent indexes in place, the evaluator can use a simple "index first" rule; a full optimiser is a separate RFC.
- **LSH / fuzzy matching** — separate concern, no current classifier needs it.
- **Partition-refinement** (Paige-Tarjan) — orthogonal RFC for structural-similarity verbs.
- **Predicate pushdown into single-variable node emission** — subsumed when the single-variable Eq predicate is indexable; the residual non-indexable case is a query-time filter.
- **Full-text / regex indexes** — Cypher's `=~` operator stays a post-filter.

---

## 3. Design

### 3.1 Data structure

`KeyspaceState` gains:

```rust
pub(crate) struct KeyspaceState {
    // ... existing fields ...

    /// Inverted indexes by (label, prop_name) → value → node set.
    /// Populated at `ingest_nodes` from the keyspace's indexes.toml spec.
    /// Empty for keyspaces whose spec declares no indexes (or legacy keyspaces).
    pub(crate) by_prop: BTreeMap<(Label, PropKey), BTreeMap<PropValue, BTreeSet<NodeIndex>>>,

    /// The spec that produced `by_prop`. Persisted alongside so a re-extract
    /// that changes the spec invalidates the keyspace's indexes but not its
    /// fact content.
    pub(crate) index_spec: IndexSpec,
}
```

`PropKey` is `String` for regular props or `ComputedKey(FnName, Vec<Arg>)` for computed keys like `last_segment(qname)`. The `IndexSpec` is the authoritative list loaded from `.cfdb/indexes.toml`.

### 3.2 Index-spec TOML

```toml
# .cfdb/indexes.toml — v0.1 shape
[[index]]
label = "Item"
prop = "qname"

[[index]]
label = "Item"
prop = "bounded_context"

[[index]]
label = "Item"
computed = "last_segment(qname)"
```

Allowlisted computed functions (v0.1): `last_segment`, possibly `signature_digest` (TBD). Extending the allowlist is an RFC-gated change (§6.8 no-ratchet).

### 3.3 Build pass

`ingest_nodes` iterates the spec and populates `by_prop` in one O(n × |indexes|) pass. Computed keys are evaluated eagerly. Post-ingest, subsequent mutations (`ingest_nodes` again, property updates) maintain the indexes incrementally — same pattern as the existing `by_label` index.

### 3.4 Evaluator integration

`eval::pattern::candidate_nodes` gains two fast paths:

1. **Label + prop literal** — `MATCH (a:Item {qname: "foo::bar"})` → `by_prop[(Item, qname)]["foo::bar"].clone()` (or `Vec::new()` if missing) instead of full `by_label[Item]` scan.
2. **Label + WHERE Eq on literal** — when the evaluator sees `MATCH (a:Item) WHERE a.qname = $x`, it detects the predicate is indexable and uses the same path. Requires a small predicate-pushdown analyser before `apply_pattern` runs.

Cross-MATCH Eq (the #178 case) becomes **posting-list intersection**: if `last_segment(a.qname) = last_segment(b.qname)` and both sides have `last_segment` indexed, the evaluator enumerates the index's buckets and emits only the pairs within each bucket. Turns 21k² ≈ 440 M into Σ|bucket|² ≈ a few thousand.

### 3.5 Wire format

JSON keyspace files gain an optional `indexes` block:

```json
{
  "schema_version": { "major": 0, "minor": 3, "patch": 0 },
  "nodes": [...],
  "edges": [...],
  "indexes": {
    "spec": [ { "label": "Item", "prop": "qname" }, ... ],
    "entries": { "Item:qname": { "foo::bar": [12, 45], ... }, ... }
  }
}
```

Absence of `indexes` block → the loader rebuilds indexes from `.cfdb/indexes.toml` (falling back to the empty spec if no file).

`SchemaVersion` bumps from `0.2.x` to `0.3.0`. Backward-compat: `0.2` keyspaces load, indexes are lazily rebuilt. Forward-compat: `0.3` keyspaces read by `0.2` binaries — **breaking**, since `0.2` doesn't know to strip the `indexes` block. Mitigate with a hard version check per §6.8.

---

## 4. Invariants

- **Determinism / byte-stable canonical dumps.** `canonical_dump` excludes the `indexes` block — it's scratch, not part of the extracted fact content. Two extracts of the same tree with the same spec produce byte-identical `canonical_dump` output regardless of whether indexes are serialised.
- **Recall.** A test harness asserts that for every indexed `(label, prop)` pair, the index produces exactly the node set a full scan would. Any divergence is a bug.
- **No-ratchet rule (§6.8).** The computed-key allowlist is `const` in `cfdb-core`. Adding a key requires a reviewed PR. Indexes.toml shape changes require RFC.
- **Keyspace backward-compat.** v0.2 keyspaces load into v0.3 binaries (indexes rebuilt). v0.3 keyspaces are rejected by v0.2 binaries with a clear schema-version error.
- **Cross-repo lockstep (RFC-033 §4 I2).** SchemaVersion bump requires a paired PR on graph-specs-rust bumping `.cfdb/cross-fixture.toml` to the cfdb PR's HEAD SHA.

---

## 5. Architect lenses

Placeholders for §2.3 review. Each lens returns a verdict (RATIFY / REJECT / REQUEST CHANGES) with evidence.

### 5.1 Clean architecture (`clean-arch`)

Question: where does the index live? Is it part of the `StoreBackend` trait or an internal petgraph detail? Does adding indexes violate the port/adapter separation?

Open points for the reviewer:
- Should `StoreBackend::execute` remain agnostic to index mechanics (indexes are internal to `PetgraphStore`)?
- Or should a new `IndexBackend` port exist, opening the door to future backend implementations (e.g. a `sled`-backed keyspace)?

### 5.2 Domain-driven design (`ddd-specialist`)

Question: does `Index` / `IndexSpec` introduce new domain vocabulary that needs bounded-context placement? Are we conflating "query-optimisation artifact" with "fact content"?

Open points:
- Should `indexes.toml` live alongside `.cfdb/concepts/*.toml` (same config directory, same authorship workflow)?
- Is "index" a concept the user writes Cypher against, or purely internal?

### 5.3 SOLID + component principles (`solid-architect`)

Question: does index logic justify its own crate (`cfdb-index`), or is it SRP-appropriate inside `cfdb-petgraph`?

Open points:
- The build pass, the lookup interface, and the evaluator integration are three concerns. One crate or split?
- `StoreBackend` abstraction remains stable (§A2.1 stable abstractions principle)?

### 5.4 Rust systems (`rust-systems`)

Question: `FxHashMap` vs `BTreeMap` for posting lists? Const-sized indexes via type-level tricks? Serialisation evolution when the spec grows?

Open points:
- `BTreeMap<PropValue, BTreeSet<NodeIndex>>` at build-time (for determinism) then converted to `FxHashMap` once frozen?
- Keyspace file size blow-up on large indexes — worth a streaming serde strategy?
- Incremental index maintenance on `ingest_nodes` when nodes update in place.

---

## 6. Non-goals

(Restated from §2 for emphasis.)

- Cost-based planner.
- LSH / approximate matching.
- Partition-refinement for structural equivalence.
- Predicate pushdown for non-indexable single-variable predicates.
- Regex / full-text indexes.

---

## 7. Issue decomposition (post-ratification)

Vertical slices, each filed as a `forge_create_issue` with `Refs: docs/RFC-035-persistent-inverted-indexes.md` and a prescribed `Tests:` block per §2.5.

1. **`IndexSpec` + `.cfdb/indexes.toml` loader** (`cfdb-core`)
   Tests: Unit on parser + serde round-trip; self dogfood on cfdb's own `.cfdb/indexes.toml`.

2. **`KeyspaceState::by_prop` + build pass** (`cfdb-petgraph`)
   Tests: Unit asserting index recall ≡ full scan on a synthetic 1 000-item keyspace; AC6-shaped determinism test on canonical dump (indexes must not leak into canonical output).

3. **Computed-key allowlist + `last_segment(qname)` evaluator** (`cfdb-core`)
   Tests: Unit on the computed function; integration test that an indexed `last_segment` lookup matches a non-indexed Cypher `last_segment()` call.

4. **Serialisation of `indexes` block + lazy rebuild on load** (`cfdb-petgraph::persist`)
   Tests: Unit on serde round-trip; load test on a legacy v0.2 keyspace (must rebuild + warn, not error); schema-version rejection test (v0.2 loading a v0.3 keyspace errors with a clear message).

5. **Evaluator fast paths** (`cfdb-petgraph::eval::pattern`)
   Tests: Unit asserting `candidate_nodes` returns the same set with/without indexes on a fixture; self dogfood (cfdb scope on cfdb) wall time < 10 s.

6. **Cross-MATCH posting-list intersection** (`cfdb-petgraph::eval`)
   Tests: Unit on the `context_homonym`-shape fixture (10 known pairs in 1 000 items, result correct + time < 100 ms); target dogfood (qbot-core 148k keyspace `cfdb scope` < 60 s).

7. **SchemaVersion bump to 0.3.0 + graph-specs-rust lockstep fixture** (`cfdb-core`, cross-repo)
   Tests: Cross dogfood on graph-specs-rust pinned SHA — zero findings delta pre/post.

Each slice carries the full `Tests:` 4-row block from §2.5. Slice 7 is merge-ordered last per RFC-033 §4 (cfdb merges, graph-specs fixture bump follows within minutes).

---

## 8. Open questions for council

- Is `.cfdb/indexes.toml` the right config location, or should indexes be declared inline in the keyspace schema header?
- Should we version the computed-key allowlist independently of the SchemaVersion?
- Cross-repo: does graph-specs-rust consume indexes, or just tolerate the new SchemaVersion?

---

## 9. Signals that RFC-035 has succeeded

- `cfdb scope --context <any>` on a 148k-node keyspace returns within 60 s.
- Peak RSS on the same workload stays < 500 MB (no regression from #168).
- Keyspace-on-disk file grows by < 20% from the indexes block on a representative cfdb extract.
- All existing eval tests pass byte-identically.
- Cross dogfood on graph-specs-rust at pinned SHA: zero findings delta.

# RFC-035 — Persistent inverted indexes on `:Item` props and computed keys

Status: **draft, R2 (post-R1 council)**
Parent trace: #167 → #168 (PR #177, merged) → #178 (closed, subsumed) → **this RFC**
Companion: #167 trial evidence — 148k-node keyspace, 428 MB RSS (fixed), 16+ min scope wall time (unfixed).

---

## 1. Problem

Empirical trial on the #167 reproducer (qbot-core-trial-4055, 148 875 nodes / 149 361 edges) after #168 merged:

| Axis | Pre-#168 | Post-#168 | After RFC-035 (target) |
|---|---|---|---|
| Peak RSS | 13.13 GB (OOM-killed) | 428 MB | < 500 MB |
| `cfdb scope --context X` wall time | N/A (OOM) | > 16 min (killed) | < 60 s |

#168 fixed memory structurally (streaming evaluator, cartesian product no longer materialised). But **CPU is unchanged**: multi-MATCH queries still walk every pair. On `context_homonym`-shaped rules (21 432² ≈ 440 M candidate pairs for a result set of hundreds), scope verbs remain unusable.

cfdb keyspaces are **write-once / read-many**: extract once, query dozens of times via `scope` / `check` / `violations`. Paying the index cost once at extract time and amortising across every subsequent query is the architecturally correct move. This RFC proposes **persistent inverted indexes**.

---

## 2. Scope

### Deliverables

1. **Per-`(Label, prop)` inverted indexes** — `by_prop[(label, prop_name)][prop_value] → BTreeSet<NodeIndex>` built during `ingest_nodes`, stored on `KeyspaceState`. **Always rebuilt on load** — not serialised to disk (see §3.7 and R1 B2 resolution).
2. **Computed-key registry** — a short allowlist of pure functions (`last_segment(qname)`, TBD others) sourced from `cfdb-core::qname` that the index build pass evaluates once per node and stores as virtual props. Indexes treat them like any other prop (§3.3).
3. **Evaluator integration** — `candidate_nodes` consults `by_prop` when the MATCH pattern binds a prop literal; cross-MATCH Eq predicates intersect posting lists (§3.6).
4. **Index-spec TOML** — `.cfdb/indexes.toml` naming which (label, prop) pairs are indexed. Adding an index is reviewed (per §6.8 no-ratchet rule). Loader lives in `cfdb-petgraph`, not `cfdb-core` (R1 B1 resolution).
5. **Composition-root wiring** — `cfdb-cli::compose::load_store` reads `.cfdb/indexes.toml` and hands `IndexSpec` to `PetgraphStore` via a builder method (§3.8).

### Non-deliverables

- **Cost-based query planner** — deferred. Simple "index first" rule only.
- **LSH / fuzzy matching** — separate concern.
- **Partition-refinement** (Paige-Tarjan) — orthogonal RFC for structural-similarity verbs.
- **Predicate pushdown into single-variable node emission** — subsumed where the Eq predicate is indexable.
- **Full-text / regex indexes** — Cypher's `=~` operator stays a post-filter.
- **Wire-format change / `SchemaVersion` bump** — R1 B2 resolution eliminates this. Indexes rebuild on load; the on-disk keyspace is bit-identical to pre-RFC-035. No RFC-033 §4 lockstep PR on graph-specs-rust required.

---

## 3. Design

### 3.1 Data structure

`KeyspaceState` (in `cfdb-petgraph`) gains:

```rust
pub(crate) struct KeyspaceState {
    // ... existing fields ...

    /// Inverted indexes by (label, prop_key) → value → node set.
    /// Populated lazily at ingest_nodes / persist::load from the keyspace's
    /// index spec. Empty for keyspaces whose spec declares no indexes.
    /// NOT serialised to disk — rebuilt on every load.
    pub(crate) by_prop: BTreeMap<(Label, PropKey), BTreeMap<PropValue, BTreeSet<NodeIndex>>>,

    /// The spec that produced `by_prop`. Held in-process; persisted indirectly
    /// via `.cfdb/indexes.toml` rather than as part of the keyspace file.
    pub(crate) index_spec: IndexSpec,
}
```

`PropKey`, `IndexSpec`, and `ComputedKey` are **defined in `cfdb-petgraph`** — not in `cfdb-core`. These are backend-optimisation artefacts with no stable abstract meaning; placing them in `cfdb-core` would violate the Stable Abstractions Principle and the crate's documented zero-I/O / zero-external-dep invariant (`crates/cfdb-core/src/lib.rs:6-7`). (R1 B1 resolution — clean-arch + solid-architect convergent concern.)

Concretely, they live in a new `crates/cfdb-petgraph/src/index/mod.rs` module:
- `index/spec.rs` — `IndexSpec`, `ComputedKey`, TOML loader
- `index/build.rs` — ingest-time build pass
- `index/lookup.rs` — `candidate_nodes` fast paths + cross-MATCH posting-list intersection

### 3.2 Index-spec TOML

```toml
# .cfdb/indexes.toml — v0.1 shape

[[index]]
label = "Item"
prop = "qname"
notes = "Join key for list-callers and find-canonical verbs; high-cardinality, always indexed."

[[index]]
label = "Item"
prop = "bounded_context"
notes = "Scope-verb filter predicate (#169 / RFC-035); low-cardinality, small index."

[[index]]
label = "Item"
computed = "last_segment(qname)"
notes = "Homonym-pair join key for context_homonym classifier rule (#48 class 2)."
```

The required `notes` string on each entry documents the rationale — who uses it, why it's indexed. Pattern-match on `.cfdb/skill-routing.toml` where every routing decision carries the same kind of rationale. (R1 R2 resolution — DDD lens.)

**Computed-function allowlist.** `last_segment(qname)` in v0.1. Extending the allowlist is an RFC-gated change (§6.8 no-ratchet). Each allowlisted function is a pure wrapper around a function in `cfdb-core::qname` — see §3.3.

### 3.3 Computed-key registry

Computed keys are **wrappers around canonical qname-formula functions in `cfdb-core::qname`**, which is cfdb's invariant owner for qname structure (`crates/cfdb-core/src/qname.rs`). Any computed key used in an index MUST:

1. Be a pure function of a `:Item`'s properties (no I/O, no allocation beyond the return value).
2. Reference a specific `cfdb-core::qname::*` helper as its semantic anchor, so the computed key's behaviour moves in lockstep with the canonical qname contract. If `cfdb-core::qname::module_qpath` changes, `last_segment(qname)` must either continue to be consistent with it or the index is invalidated and the computed-key registry entry is updated in the same PR.
3. Be evaluated at ingest time and stored as a virtual prop on the node, tagged by the computed-function name. Subsequent reads see it as any other prop value.

For v0.1, `last_segment(qname)` splits at the last `::` in the qname string — semantically consistent with qname-path grammar established by `cfdb-core::qname` (syn extractor and HIR extractor both use this module; cross-extractor edge landing depends on it). A corresponding `pub fn last_segment(qname: &str) -> &str` helper lands in `cfdb-core::qname` as part of slice 3 (§7) to make the invariant explicit. (R1 B3 resolution — DDD lens.)

### 3.4 Registry closure: const allowlist, not open trait

The computed-key registry is a `const`-sized enum with compile-time dispatch, not an open `HashMap<FnName, Box<dyn Fn>>` or trait-based registry. Rationale:

- **Expected cardinality.** v0.1 ships with one key (`last_segment(qname)`). The reasonably-anticipated ceiling is 3–5 over the life of the v0.2 / v0.3 schema. Opening an extensible trait surface for 3 entries is over-abstraction and re-entry-point risk (a downstream consumer could register a key that violates invariant 2 from §3.3).
- **Locked to qname contract.** Every computed key is tied to `cfdb-core::qname` per §3.3; the invariants are enforced by code review, not by trait contract. Code review is easier on a `match` statement than on an extensible trait-object registry.
- **Determinism.** A `match` on a `ComputedKey` enum compiles to a predictable LLVM jump-table; determinism across compiler versions is easier to reason about than dynamic dispatch through a `Box<dyn Fn>`.
- **If cardinality exceeds 5**, re-open this decision in a follow-up RFC. The `const` approach is not one-way: migrating from `match` to a trait registry is a mechanical refactor gated by its own RFC discussion.

(R1 B4 resolution — solid-architect lens.)

### 3.5 Build pass

`ingest_nodes` iterates the spec and populates `by_prop` in one O(n × |indexes|) pass. Computed keys are evaluated eagerly. Post-ingest, subsequent mutations (re-`ingest_nodes`, property updates) maintain the indexes incrementally — same pattern as the existing `by_label` index.

**Stale-entry removal on re-ingest** is explicit: if a re-ingested node changes a prop value that is indexed, the old value's posting-list entry for that NodeIndex is removed before the new one is inserted. (R1 R4 resolution — rust-systems lens.) Test prescription covers this in slice 2.

### 3.6 Evaluator integration

`eval::pattern::candidate_nodes` gains two fast paths:

1. **Label + prop literal** — `MATCH (a:Item {qname: "foo::bar"})` → `by_prop[(Item, qname)]["foo::bar"].clone()` (or empty) instead of full `by_label[Item]` scan.
2. **Label + WHERE Eq on literal** — when the evaluator sees `MATCH (a:Item) WHERE a.qname = $x`, it detects the predicate is indexable and uses the same path.

Cross-MATCH Eq — the #178 case — becomes **posting-list intersection**: if `last_segment(a.qname) = last_segment(b.qname)` and both sides have `last_segment` indexed, the evaluator enumerates the index's buckets and emits only the pairs within each bucket. Turns 21k² ≈ 440 M into Σ|bucket|² ≈ a few thousand.

### 3.7 Wire format (no change)

Per R1 B2 resolution (rust-systems): the wire format is **unchanged**. Indexes are NOT serialised to disk. Every `persist::load` rebuilds `by_prop` from the in-memory fact content and the current `.cfdb/indexes.toml`.

Rationale: `petgraph::StableDiGraph::NodeIndex` values are ephemeral. `persist::load` re-ingests nodes in `(label, id)`-sorted order, not extract order, so serialised `NodeIndex` integers would misdirect after round-trip — a soundness bug. Rebuilding on load trades a one-time O(n × |indexes|) rebuild cost for soundness. Rebuild cost on a 148k-node keyspace with 3 indexes is a few hundred ms, negligible against query wall times.

**Consequence:** no `SchemaVersion` bump, no paired PR on graph-specs-rust per RFC-033 §4 lockstep. Legacy v0.2 keyspaces load bit-identically.

### 3.8 Composition-root wiring

`cfdb-cli::compose::load_store(workspace_root)` is the sole loader path. It:

1. Reads `.cfdb/indexes.toml` from the workspace root via the new `cfdb_petgraph::index::spec::load_from_toml(&Path)` → `Result<IndexSpec>`. Missing file is not an error — returns `IndexSpec::empty()`.
2. Constructs `PetgraphStore::new().with_workspace(&root).with_indexes(index_spec)`. `with_indexes` is a new builder method, symmetric to the existing `with_workspace`.
3. Subsequent `PetgraphStore::ingest_nodes` / `persist::load` consult `self.index_spec` and populate `by_prop` on the relevant `KeyspaceState`.

The CLI composition root is the only place that reads TOML → `IndexSpec`. Lower layers receive `IndexSpec` values ready to use. (R1 R1 resolution — clean-arch lens.)

---

## 4. Invariants

- **Determinism / byte-stable canonical dumps.** `canonical_dump` excludes any index-related state — indexes are rebuild-able scratch. Two extracts of the same tree produce byte-identical `canonical_dump` output.
- **Keyspace backward-compat.** The wire format is unchanged (R1 B2 resolution). Any legacy keyspace loads cleanly; indexes rebuild on load from the current `.cfdb/indexes.toml` (empty spec if the file is absent).
- **Recall.** A test harness asserts that for every indexed `(label, prop)` pair, the index produces exactly the node set a full scan would. Any divergence is a bug.
- **No-ratchet rule (§6.8).** The computed-key allowlist is `const` in `cfdb-petgraph`. Adding a key requires a reviewed PR referencing this RFC.
- **`cfdb-core::qname` as invariant owner.** Every computed key is a wrapper around a `cfdb-core::qname` helper; the qname contract flows unchanged through the index subsystem.
- **Stable abstractions (SAP).** `cfdb-core` is untouched by this RFC save for the addition of `pub fn last_segment(&str) -> &str` in `qname.rs` (slice 3). `StoreBackend` trait is untouched (no `IndexBackend` port — R1 B5 resolution).
- **Cross-repo coordination.** Not applicable — no `SchemaVersion` bump, no paired PR on graph-specs-rust.

---

## 5. Council review

### 5.1 R1 (2026-04-22) — REQUEST CHANGES

All four §2.3 lenses reviewed the R1 draft.

| Lens | Verdict | Primary concern |
|---|---|---|
| clean-arch | REQUEST CHANGES | `IndexSpec` loader placed in `cfdb-core` (zero-I/O violation) |
| ddd-specialist | REQUEST CHANGES | `last_segment(qname)` not tied to `cfdb-core::qname` as invariant owner |
| solid-architect | REQUEST CHANGES | `IndexSpec` / `ComputedKey` in `cfdb-core` violates SAP |
| rust-systems | REQUEST CHANGES | `NodeIndex` serialisation is a soundness bug |

Five BLOCKING items identified and addressed in this R2 draft:

| # | Item | R2 resolution |
|---|---|---|
| B1 | Move `IndexSpec` / `ComputedKey` / loader to `cfdb-petgraph` | §2 deliverable 4; §3.1 |
| B2 | Drop `entries` block from wire format; always rebuild on load | §3.7 |
| B3 | Tie `last_segment(qname)` to `cfdb-core::qname` | §3.3 |
| B4 | OCP decision section (const vs trait-registry) | §3.4 |
| B5 | Close §5.1 `IndexBackend` open question with "no" | §4 (recorded); removed from §8 |

Four REQUEST-CHANGE items (R1–R4) also resolved: §3.8 composition-root wiring; §3.2 `notes` field; §8 config location; §3.5 stale-entry removal test prescription (slice 2).

Detailed lens verdicts are in the local (gitignored) `council/035/` directory at the time of authoring — available to reviewers on the `rfc/035-persistent-inverted-indexes` branch checkout.

### 5.2 R2 — pending

Council re-convenes on R2 submission. Expected outcome if all five BLOCKING items are correctly addressed: RATIFY. Otherwise R3.

---

## 6. Non-goals

Restated from §2 for emphasis.

- Cost-based planner.
- LSH / approximate matching.
- Partition-refinement for structural equivalence.
- Predicate pushdown for non-indexable single-variable predicates.
- Regex / full-text indexes.
- On-disk index serialisation (R1 B2 eliminated this from scope).
- `SchemaVersion` bump / graph-specs-rust cross-repo lockstep PR (R1 B2 consequence).

---

## 7. Issue decomposition (post-ratification)

Vertical slices, each filed with `Refs: docs/RFC-035-persistent-inverted-indexes.md` and a prescribed `Tests:` block per §2.5.

1. **`IndexSpec` + `.cfdb/indexes.toml` loader** (`cfdb-petgraph::index::spec`)
   Tests: Unit on parser + serde round-trip; self dogfood on cfdb's own `.cfdb/indexes.toml`; recall of `notes` field through round-trip.

2. **`KeyspaceState::by_prop` + build pass + stale-entry removal on re-ingest** (`cfdb-petgraph::index::build`)
   Tests: Unit asserting index recall ≡ full scan on a synthetic 1 000-item keyspace; AC6-shaped determinism test on canonical dump (indexes must not leak); **stale-entry test** — re-ingest a node with a changed indexed prop, assert old value's posting-list entry is removed AND new value's is present.

3. **Computed-key allowlist + `last_segment(qname)` in `cfdb-core::qname`** (`cfdb-core`, `cfdb-petgraph::index::spec`)
   Tests: Unit on `cfdb-core::qname::last_segment` (pure function, round-trip with `module_qpath`); integration test asserting indexed `last_segment` lookup matches a non-indexed Cypher `last_segment()` call byte-for-byte.

4. **Lazy rebuild on `persist::load`** (`cfdb-petgraph::persist`, `cfdb-petgraph::index`)
   Tests: Round-trip a keyspace through `save` + `load`, assert `by_prop` is populated post-load and matches ingest-time state; legacy v0.2 keyspace load test (must succeed with rebuilt indexes, no warning).

5. **Evaluator fast paths — label + prop literal, label + WHERE Eq on literal** (`cfdb-petgraph::eval::pattern`, `cfdb-petgraph::index::lookup`)
   Tests: Unit asserting `candidate_nodes` returns the same set with/without indexes on a fixture; self dogfood (cfdb scope on cfdb) wall time < 10 s.

6. **Cross-MATCH posting-list intersection** (`cfdb-petgraph::eval`, `cfdb-petgraph::index::lookup`)
   Tests: Unit on the `context_homonym`-shape fixture (10 known pairs in 1 000 items, result correct + time < 100 ms); target dogfood — `cfdb scope --context <any>` on the 148k qbot-core-trial-4055 keyspace completes in < 60 s with peak RSS < 1 GB.

7. **Composition-root wiring** (`cfdb-cli::compose`)
   Tests: Unit asserting `load_store` reads `.cfdb/indexes.toml` and hands the spec to the store; end-to-end test asserting `cfdb scope` on a workspace with an indexes.toml exercises the index path (observable via a debug counter or a new `--explain` flag — TBD during slice 5 implementation).

Each slice carries the full `Tests:` 4-row block from §2.5. Slices are merge-ordered top-to-bottom. Slice 3 must land before slice 5 so the qname helper exists when the evaluator fast paths exercise it.

---

## 8. Open questions (R2)

All R1 open questions resolved:

- **Config location** — answered §3.2: `.cfdb/indexes.toml` adjacent to `.cfdb/skill-routing.toml`, not inline in keyspace schema header.
- **Computed-key allowlist versioning** — answered §6.8 / §3.4: versioned with the RFC that adds the key, never independently.
- **Cross-repo graph-specs-rust** — answered §4: no SchemaVersion bump, no paired PR.
- **`IndexBackend` port** — answered §4: no, indexes are internal to `cfdb-petgraph`.

No new open questions introduced in R2.

---

## 9. Signals that RFC-035 has succeeded

- `cfdb scope --context <any>` on a 148k-node keyspace returns within 60 s.
- Peak RSS on the same workload stays < 500 MB (no regression from #168).
- Legacy v0.2 keyspaces load without warnings or errors.
- Keyspace-on-disk file size is unchanged (no `entries` block written).
- All existing eval tests pass byte-identically.
- Cross dogfood on graph-specs-rust at pinned SHA: zero findings delta.

# RFC-046 — Runtime execution-trace ingest (`:Trace` + `OBSERVED_CALLS`)

- **Status:** RATIFIED. See §5.
- **Issue:** #477
- **Schema impact:** new node label `:Trace`, new edge label `OBSERVED_CALLS` → **minor bump `SchemaVersion::V0_6_0`**. (No new `resolver` value — see §5.2.)
- **Companion:** lockstep `.cfdb/cross-fixture.toml` bump on `graph-specs-rust` (cfdb-033-cross-dogfood#4)

---

## 1. Problem

cfdb is 100% static. Static reachability answers *what could run*; it cannot answer *what did run*, with weights, scoped to a single execution. The motivating reference is Blackfire: a call tree scoped to one profiled request, each node carrying inclusive/exclusive cost. Neither cfdb nor comparable static tools (graphify) can produce this — it is structurally impossible without a dynamic fact source. That makes it cfdb's strongest *differentiating* capability rather than a catch-up feature.

Three concrete queries are unavailable today and become available with a runtime tier:

1. **Scoped, weighted call subtree** — "what actually executed under entry point X, and where did the cost go?"
2. **Static-graph recall check** — observed edges with no matching static `CALLS` edge expose dynamic dispatch / reflection / macro-generated calls the extractor never saw. No purely-static tool can surface this.
3. **Dead-under-real-traffic** — static `CALLS` edges never observed across a trace corpus.

## 2. Scope

**Ships (v1):**
- New schema vocabulary: `:Trace` node, `OBSERVED_CALLS` edge. `SchemaVersion::V0_6_0`.
- An **inherent ingest method on `PetgraphStore`** (not a new cfdb-core trait — §3.3) that **ingests profiler output into an already-extracted keyspace** — it does not run a profiler.
- **One input format, deeply: folded/collapsed stacks** (Brendan Gregg format: `frameA;frameB;frameC <count>`) — the universal substrate emitted by `perf`, `py-spy`, `0x`, and `cargo flamegraph`, line-trivial to parse, dependency-free.
- CLI verb `cfdb ingest-trace --db … --keyspace … --trace-file <path> --format folded [--label …]`.
- Frame → `:Item` resolution (closed-world: unresolved frames dropped-and-counted, never materialized as synthetic nodes).
- A dogfood **coverage gate** (resolution ratio), not a recall-corpus extension.

**Does not ship (explicit, see §6):** a profiler/instrumentation; wall-clock/memory formats (cachegrind, chrome-trace) — deferred to cfdb-046-runtime-trace-ingest-D; cross-keyspace queries; variable-length scoped traversal; synthetic nodes for unresolved frames.

## 3. Design

### 3.1 Schema vocabulary (cfdb-core)

`Label` / `EdgeLabel` are `#[serde(transparent)]` newtypes over `String` (`crates/cfdb-core/src/schema/labels.rs:20-22,112-114`) — open sets by construction. We add **two** string consts:

- `Label::TRACE = "Trace"` and `EdgeLabel::OBSERVED_CALLS = "OBSERVED_CALLS"`.

**No new `resolver` value.** `resolver` is a *static-analysis-producer* discriminator (`{syn, hir, tree-sitter-php, tree-sitter-typescript}`) guarded by a live ban rule (`examples/queries/arch-ban-rfc-043-resolver-domain.cypher`). A profiler **observes**; it does not resolve. The runtime tier is distinguished by its **edge label** (`OBSERVED_CALLS` vs `CALLS`) and carries a `profiler` prop naming the tool — it never touches `resolver`.

The new **labels** force the bump. New-fact-type precedent (V0_2_0 EntryPoint, V0_4_0 `:Literal`, V0_5_0 `:Argument`) is a **minor** bump → `SchemaVersion::V0_6_0`, `CURRENT` repointed; `can_read()` (`labels.rs:478-480`) then refuses a V0_6_0 graph to a V0_5_0 reader.

`PropValue` already carries `Int(i64)` (`fact.rs:17-26`); all v1 metrics are integer sample counts, so no `PropValue` change. (`From<f64>` is absent — a *deliberate accepted gap*; cfdb-046-runtime-trace-ingest-D wall-time props will write `PropValue::Float(x)` explicitly.)

### 3.2 Node and edge shape

**`:Trace` node** — one per profiled execution (the *scope*). `id = trace_id` = **sha256** of the trace file bytes + entry frame (§I8; sha256 matches cfdb precedent at `commands/extract.rs:388` — never `DefaultHasher`, whose seed is non-portable). Props:

| prop | type | meaning |
|---|---|---|
| `entry_qname` | Str | resolved qname of the observed root frame |
| `profiler` | Str | `perf` / `py-spy` / `cargo-flamegraph` / … |
| `format` | Str | `folded` (v1) |
| `samples_total` | Int | total samples in the trace |
| `run_label` | Str | caller-supplied (`--label`), optional |
| `captured_at` | Int | unix ts **only if present in the input**; never synthesized |

**`OBSERVED_CALLS` edge** — `:Item`(caller) → `:Item`(callee), bag semantics (no edge identity, matching `CALLS`). Props:

| prop | type | meaning |
|---|---|---|
| `trace_id` | Str | FK to the `:Trace` node id; **also the canonical-dump sort tiebreaker (§I3)** |
| `profiler` | Str | mirrors the trace's profiler (provenance of the observation tool) |
| `samples` | Int | sample count attributed to this caller→callee edge |
| `self_samples` | Int | samples where callee is a leaf in this edge's context |

> **YAGNI on the join.** `:Trace` ↔ `OBSERVED_CALLS` is linked by the `trace_id` **prop**, not a second edge label — avoiding a third vocabulary item and its edge-liveness/`Reserved` burden. An explicit `:Trace-[:ROOTS]->:Item` edge is a documented cfdb-046-runtime-trace-ingest-D extension if traversal ergonomics demand it. `:Trace` is a distinct aggregate from `:EntryPoint`; `entry_qname` as a cross-context *prop* — not an edge — is correct because the runtime root may not resolve to any declared `:EntryPoint`.

### 3.3 Ingest mechanism — inherent `PetgraphStore` method, **no new cfdb-core trait**

R1/clean-arch + solid rejected the draft's `TraceBackend` cfdb-core trait. Two findings drove it: (a) the proposed signature leaked `&Path` (std filesystem) into the dependency-free inner ring, which `EnrichBackend` deliberately avoids (every method takes only `&Keyspace`; the path lives as adapter state via `PetgraphStore::with_workspace`, `crates/cfdb-petgraph/src/lib.rs:112-114`, wired at the composition root `cfdb-cli/src/compose.rs:139-155`); (b) once the path is injected adapter-side, the *only* stated reason for a separate trait evaporates — and cfdb-core is maximally stable (I=0, Ca=11): a third trait forces 11-crate recompiles on every edit, for **one implementor and one consumer**.

**Decision — Option C (considered A/B/C):**

| | placement | cfdb-core blast-radius | chosen |
|---|---|---|---|
| A | new `TraceBackend` trait in cfdb-core | 11 crates recompile per edit | ✗ over-segregated; no 2nd implementor exists |
| B | method on `EnrichBackend` | core edit, but stub-defaulted | ✗ trace-ingest is a 2nd *ingest source*, not an annotation pass |
| **C** | **inherent `PetgraphStore::ingest_trace`** | **zero (petgraph + cli only)** | **✓** |

Mirrors the existing inherent `PetgraphStore::execute_explained` (`crates/cfdb-petgraph/src/lib.rs:189-211`) which is deliberately *not* on `StoreBackend`. If a second backend (SQLite/Kùzu) ever needs trace-ingest, extract the trait **then** (at the second implementor — the rule-of-three), not speculatively.

```rust
// crates/cfdb-petgraph/src/trace/mod.rs (new) — inherent on PetgraphStore
impl PetgraphStore {
    pub fn with_trace_file(self, path: PathBuf) -> Self { /* builder, like with_workspace */ }
    pub fn ingest_trace(&mut self, keyspace: &Keyspace, format: TraceFormat)
        -> Result<TraceReport, StoreError>;   // reads self.trace_file, no &Path in any signature
}
```

`TraceReport` is a **distinct struct in cfdb-petgraph** (`ran / frames_scanned / nodes_written / edges_written / unresolved_frames / warnings`) — not an `EnrichReport` alias. `TraceFormat` (`#[non_exhaustive]`, v1: `Folded`) **also lives in cfdb-petgraph**, resolving the OCP concern (§5.3): adding cachegrind/chrome-trace in cfdb-046-runtime-trace-ingest-D adds a `FormatParser` strategy impl in cfdb-petgraph and never edits cfdb-core or any trait. Emission uses the existing `ingest_nodes`/`ingest_edges` (additive — G3); `StoreBackend` is untouched (it stores arbitrary label strings + props).

### 3.4 Frame → `:Item` resolution (closed-world)

A folded frame is a language-shaped symbol (`crate::mod::fn`, `Class::method`, `file.ts:fn`). Resolution ladder, O(1) per frame via the existing qname index `KeyspaceState.id_to_idx` (`crates/cfdb-petgraph/src/graph.rs`; same index the call-resolution pass uses, `enrich/attr_call_resolution.rs:165-166`):

1. exact `:Item.qname` match;
2. normalized match (strip hash suffixes `::h1a2b…`, monomorphization tags, generics);
3. unresolved → **dropped, counted** in `TraceReport.unresolved_frames`.

Per [[feedback_stubs_not_arrows]] and the PHP/TS closed-world precedent (IMPLEMENTS emits only when the target resolves in-workspace, `cfdb-extractor-php/src/emitter.rs:104-119`), unresolved frames do **not** become `_external`/`_synthesized` stub nodes. The resolution ratio is the coverage signal (§3.6).

### 3.5 CLI — dedicated dispatch branch (not `dispatch_enrich`)

R1/rust-systems found `dispatch_enrich` bottlenecks into `enrich(db, keyspace, verb, workspace)` with **payload-free** `EnrichVerb` variants (`crates/cfdb-cli/src/main_dispatch.rs:226-275`) — it structurally cannot carry `trace_file`/`format`. So:

- `Command::IngestTrace { db, keyspace, trace_file, format, label?, workspace? }` (`main_command/args.rs`).
- A **dedicated `dispatch_trace` arm** in `main.rs`/`main_dispatch.rs` (not the enrich group).
- Handler `pub fn ingest_trace(...)` in **new `crates/cfdb-cli/src/trace.rs`**: `compose::load_store_with_workspace` → `store.with_trace_file(path)` → `store.ingest_trace(keyspace, format)` → conditional `compose::save_store` (persist only if `ran && (nodes_written>0 || edges_written>0)`, matching enrich). Re-exported from the crate root for `main_dispatch.rs`.

### 3.6 Coverage gate (the recall substitute)

rustdoc-json is **no oracle** for runtime edges (cfdb-037-schema-producer-alignment retired recall-corpus extension for non-rustdoc vocabulary; `cfdb-recall` measures `:Item` nodes only). We **do not** touch the recall corpus. Instead we reuse the **dogfood-enrich coverage gate**: a `const` floor in tool source (no baseline/allowlist file — §6 rule 8) enforced by a zero-rows Cypher sentinel.

- `OBSERVED_CALLS_RESOLUTION_THRESHOLD: Option<u32> = Some(N)` in `tools/dogfood-enrich/src/thresholds.rs` (alongside `BC_COVERAGE_THRESHOLD` etc., :36-69 — the file's own header records why thresholds live here and not in cfdb-core/cfdb-cli).
- `.cfdb/queries/self-enrich-trace.cypher` — returns rows (→ exit 30) when the share of `OBSERVED_CALLS` edges whose endpoints are known `:Item`s drops below the floor, via the `ratio_substitutions` path (`tools/dogfood-enrich/src/main.rs:226-264`).
- Paired with an **exact-count golden-fixture** test (PHP/TS precedent): a committed synthetic folded-stacks fixture whose frames match known cfdb-self qnames, asserting an exact `OBSERVED_CALLS` count + resolution at **PR time** (no live profiler in CI). The "profile a real cfdb run" assertion is **nightly**.

### 3.7 Module split (SRP)

Three independently unit-testable units under `crates/cfdb-petgraph/src/trace/`:
- `parser.rs` — pure `bytes → Vec<(frames, count)>` (per-format; `FormatParser` strategy, §3.3);
- `resolver.rs` — pure `(frames, &id_index) → resolved (caller,callee,samples)` triples + unresolved count;
- `emitter.rs` — `resolved → Vec<Node>/Vec<Edge>` batches + the `:Trace` node.

`ingest_trace` (`mod.rs`) wires the three. The 46-B `Tests:` block pins parser and resolver as isolated pure-function units.

## 4. Invariants

- **I1 — Determinism (G1) preserved by construction.** `ci/determinism-check.sh` runs `cfdb extract` then `cfdb dump`; it **never** runs `ingest-trace`. Runtime facts exist only after the opt-in post-extract verb, so extract→dump byte-stability is structurally untouched. No attribute-exclusion mechanism is introduced.
- **I2 — Ingest sub-contract (G6).** `ingest_trace` is a pure function of `(keyspace, trace_file, format)`: re-ingesting the same trace file onto the same keyspace is byte-stable. All v1 metrics are **integer** sample counts read verbatim from the file (no floats in v1). Non-determinism exists only across *different profiling runs* — correct, those are different observations; `cfdb diff` shows metric deltas across runs by design.
- **I3 — Edge sort tiebreaker (a 46-B deliverable, not yet in code).** The current `canonical_dump` edge key is the 3-tuple `(label, src_qname, dst_qname)` (`crates/cfdb-petgraph/src/canonical_dump.rs:74-92`). 46-B **changes** it to the 4-tuple `(label, src_qname, dst_qname, trace_id_or_empty)`, 4th slot = `edge.props.get("trace_id").and_then(PropValue::as_str).unwrap_or("")`. The props are available at that site (`Edge.props`, `crates/cfdb-core/src/fact.rs:169`; `PropValue::as_str` at `fact.rs:29-33`). Non-`OBSERVED_CALLS` edges (no `trace_id`) sort with `""` → behaviour unchanged for existing edges. **Numeric metrics never enter the sort key.**
- **I4 — Facts vs observations.** Edge *presence* + integer *sample counts* are facts (stable per fixed input). Wall-time/memory (cfdb-046-runtime-trace-ingest-D) are *observations* — carried as props, never in any equivalence key.
- **I5 — Edge-liveness.** `OBSERVED_CALLS` does not emit on the plain `extract` self keyspace, so it is registered `Provenance::Reserved` in cfdb-core **and** added to the hardcoded `EDGE_LABELS` array in `ci/edge-liveness.sh` (both required — blocking gate since #385).
- **I6 — Schema lockstep.** `V0_6_0` requires a companion draft PR on `graph-specs-rust` bumping `.cfdb/cross-fixture.toml` to this PR's HEAD SHA (cfdb merges first; exit-20 window expected — cfdb-033-cross-dogfood#3.3). New `pub const`s also require an entry in cfdb's own `specs/concepts/cfdb-core.md` (`make graph-specs-check`).
- **I7 — No metric ratchets.** `OBSERVED_CALLS_RESOLUTION_THRESHOLD` is a `const` in tool source; no baseline/ceiling/allowlist file (§6 rule 8).
- **I8 — Portable `trace_id`.** sha256 (matching cfdb precedent), never `DefaultHasher` — so the `:Trace`↔`OBSERVED_CALLS` FK is identical across machines/arch.

## 5. Architect lenses (R1 verdicts + resolutions)

### 5.1 Clean architecture (`clean-arch`) — REQUEST CHANGES → resolved
- **Blocking: `&Path` in the cfdb-core port.** Resolved by §3.3 Option C — no cfdb-core trait at all; trace path is adapter state via `with_trace_file`, mirroring `with_workspace`. Port boundary stays filesystem-free.
- Non-blocking: `TraceReport` type identity → now a distinct cfdb-petgraph struct (§3.3); signature-pin test → 46-B condition.

### 5.2 Domain-driven design (`ddd-specialist`) — REQUEST CHANGES → resolved
- **Blocking: `resolver="runtime"` is a homonym** colliding with `arch-ban-rfc-043-resolver-domain.cypher` (`resolver` = static producer). Resolved: `resolver` **removed** from the runtime tier entirely (§3.1/§3.2); the `OBSERVED_CALLS` label + `profiler` prop carry the distinction.
- Confirmed: `OBSERVED_CALLS` as a separate label (vs a flag on `CALLS`) is correct DDD — observed-execution and declared-structure are distinct concepts; `:Trace` ≠ `:EntryPoint`; closed-world drop matches PHP/TS.

### 5.3 SOLID / components (`solid-architect`) — REQUEST CHANGES → resolved
- **Blocking: new cfdb-core trait under-justified** (Zone-of-Pain, Ca=11, one impl/one consumer). Resolved by §3.3 Option C (inherent method, zero core blast-radius) with A/B/C recorded.
- **Blocking: `TraceFormat` OCP.** Resolved: `TraceFormat` + `FormatParser` strategy live in cfdb-petgraph; new formats never edit core/trait. `#[non_exhaustive]` declared at birth.
- Confirmed: threshold placement in `tools/dogfood-enrich` correct (SDP/SAP).

### 5.4 Rust systems (`rust-systems`) — REQUEST CHANGES → resolved
- **Blocking: hash unspecified** → I8 sha256.
- **Blocking: sort tiebreaker under-specified** → I3 names the 4-tuple + `unwrap_or("")` fallback.
- **Blocking: `dispatch_enrich` can't carry payload** → §3.5 dedicated `dispatch_trace` + `trace.rs` handler.
- Non-blocking folded in: "floats" wording removed (v1 all-Int, I2); `From<f64>` noted as accepted gap (§3.1); O(1) resolution confirmed (§3.4); `runner.rs` citation corrected to `main.rs:226-264` (§3.6); `#[non_exhaustive] TraceFormat` (§3.3).

> All four lenses' blocking items are resolved with mechanisms verifiable against current code. Residual items are 46-A/46-B implementation checklist entries, not design defects.

## 6. Non-goals

- **Not a profiler.** cfdb ingests existing profiler output; producing the trace is the user's job (`cargo flamegraph`, `perf`, `py-spy`, Xdebug).
- **No wall-time / memory in v1.** Folded stacks give sample counts only. cachegrind/Xdebug + chrome-trace (inclusive/exclusive wall-time + memory — the full Blackfire shape) are **cfdb-046-runtime-trace-ingest-D**.
- **No synthetic nodes** for unresolved frames (closed-world, §3.4).
- **No cross-keyspace queries**; **no variable-length scoped traversal** in v1 (the Cypher subset does not bind edge vars in `[*1..N]`). The rooted-tree assembly (`cfdb trace-tree --entry`) using the in-process library is **cfdb-046-runtime-trace-ingest-D**.
- **No change to the `extract`/`enrich` determinism gates.**

## 7. Issue decomposition

### 46-A — Reserve the runtime vocabulary (schema + lockstep)
`Label::TRACE`, `EdgeLabel::OBSERVED_CALLS` (`Provenance::Reserved`), `SchemaVersion::V0_6_0`, node/edge descriptors, `specs/concepts/cfdb-core.md` entry, `EDGE_LABELS` array update, **and update the Reserved-set test assertion** (`crates/cfdb-core/src/schema/describe/tests.rs:248-254` currently asserts only `EQUIVALENT_TO` is `Provenance::Reserved`). No producer yet (mirrors cfdb-040-const-table-overlap slice-1 reservation). **No `resolver` value reserved.**
```
Tests:
  - Unit: const + describe round-trip; can_read(V0_5_0, V0_6_0) == false.
  - Self dogfood: edge-liveness passes with OBSERVED_CALLS Reserved; `cfdb extract --workspace .` sha unchanged (determinism-check green).
  - Cross dogfood: ci/cross-dogfood.sh vs graph-specs pinned SHA → zero rows; companion .cfdb/cross-fixture.toml bump PR open (exit-20 window documented).
  - Target dogfood: report V0_6_0 schema-describe diff in PR body.
```

### 46-B — Folded-stacks ingest (inherent method + parser/resolver/emitter + verb)
`PetgraphStore::ingest_trace` + `with_trace_file`, `TraceFormat::Folded` + `FormatParser` (cfdb-petgraph), `trace/{parser,resolver,emitter}.rs` (§3.7), the **I3 canonical_dump 4-tuple sort-key change**, `cfdb ingest-trace` verb + `dispatch_trace` + `cfdb-cli/src/trace.rs`. Flips `OBSERVED_CALLS` from Reserved to emitted. Add a signature-pin for the new public surface. Emitter: `debug_assert_eq!` the edge `profiler` equals the `:Trace.profiler` it FK-references.
```
Tests:
  - Unit: parser (pure text→frames+counts); resolver ladder (exact/normalized/unresolved) on fixed inputs; canonical_dump sorts stably for two `OBSERVED_CALLS` edges with identical `(label, src_qname, dst_qname)` but distinct `trace_id`.
  - Self dogfood: committed synthetic folded fixture over cfdb-self qnames → exact OBSERVED_CALLS count + resolution; `cargo flamegraph` a `cfdb extract` run → ingest → `MATCH (t:Trace)…` returns >0 (nightly).
  - Cross dogfood: none — rationale: trace-ingest is an opt-in post-extract verb, absent from the companion's default-feature extract path.
  - Target dogfood: report resolved/unresolved frame ratio on a qbot-core profile in the PR body.
```

### 46-C — Coverage gate
`OBSERVED_CALLS_RESOLUTION_THRESHOLD` const, `.cfdb/queries/self-enrich-trace.cypher` zero-rows sentinel, dogfood-enrich wiring (nightly job).
```
Tests:
  - Unit: threshold-pin test (mirrors thresholds.rs:86-116).
  - Self dogfood: sentinel returns zero rows at/above floor, ≥1 row below (exit 30).
  - Cross dogfood: none — rationale: coverage gate is cfdb-self only.
  - Target dogfood: report resolution ratio metric in PR body.
```

### 46-D — (Future, not v1) Wall-time formats + rooted-tree verb
cachegrind/Xdebug + chrome-trace `FormatParser`s (inclusive/exclusive wall-time + memory props — needs `From<f64>` + the dump float path), `:Trace-[:ROOTS]->:Item`, `cfdb trace-tree --entry` rooted-subtree assembly. Separate RFC amendment; listed so the v1 surface is understood as a deliberate slice. (Note: 046-D float props are constructed directly via `PropValue::Float(x)`, **not** through `fact.rs:67`'s `from_json` path — that seam is query-input only, so no f64-coercion hardening is owed here.)

---

### Appendix A — example queries (v1)

```cypher
-- Observed callees of an entry, ranked by samples
-- (RETURN the ordered attr — the evaluator binds ORDER BY to projected aliases only)
MATCH (caller:Item)-[c:OBSERVED_CALLS]->(callee:Item)
WHERE caller.qname =~ ".*::extract_workspace$"
RETURN callee.qname, c.samples
ORDER BY c.samples DESC LIMIT 20

-- Static-graph recall check: observed edges the static extractor never saw
MATCH (a:Item)-[o:OBSERVED_CALLS]->(b:Item)
WHERE NOT EXISTS { MATCH (a)-[:CALLS]->(b) }
RETURN a.qname, b.qname, o.samples

-- Dead-under-traffic: declared calls never observed across the trace corpus
MATCH (a:Item)-[:CALLS]->(b:Item)
WHERE NOT EXISTS { MATCH (a)-[:OBSERVED_CALLS]->(b) }
RETURN a.qname, b.qname
```

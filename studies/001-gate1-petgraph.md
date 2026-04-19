# Gate 1 — petgraph-baseline

**Candidate:** petgraph-baseline — hand-built openCypher subset interpreter over `petgraph::Graph`
**Query language:** in-process Rust API + a minimal Cypher-subset parser (author-defined)
**Version evaluated:** `petgraph` 0.6.x (stable), + author-written interpreter ≈ 0 LOC today
**Docs sources:**
- https://docs.rs/petgraph/latest/petgraph/ (stable API)
- https://crates.io/crates/petgraph
- Internal reasoning — this candidate is a *design-and-build*, not a *depend-on-existing*
**Rust binding:** `petgraph` is pure Rust; the Cypher-subset interpreter is also pure Rust (to be written by cfdb v0.1 author)
**Date:** 2026-04-13

## What this candidate is

petgraph-baseline is the explicit **"none of the off-the-shelf candidates fit"** fallback. It is NOT an existing graph store. It is a decision to:

1. Use `petgraph::Graph<Node, Edge>` (or `StableGraph`) as the in-memory store, with our own `Node` / `Edge` enums keyed by label + property map.
2. Write a minimal interpreter for a **subset** of openCypher: the subset must support exactly the 9 query features in §4.1, nothing more.
3. Parse Cypher with `nom` or `chumsky` (small footprint) or skip parsing entirely and expose the API as a Rust builder.
4. Persist via serde-JSONL dump (already the canonical fixture per RFC §12.1 — the store file is a cache, so persistence is cheap to implement).
5. Pay the full engineering cost of building an interpreter instead of depending on one.

**Effort estimate:** 2–3 weeks of focused work for a working subset, per RFC §10.1 and the methodology §2 fallback language. This is the fallback cost the study accepts as "worst case" — everything else must beat this.

Gate 1 scoring here is therefore unusual: every row can be "yes, we can build it", but the right question is **"how much of the 9-row grid is trivial to implement in-tree?"** — where trivial means "a half-day of Rust, no research required".

## Scoring rubric applied to a build-your-own

- **1** = trivially implementable in `petgraph` + `std` with a half-day each. No novel algorithmic work.
- **0.5** = implementable but needs a non-trivial chunk (a day or more) or an external crate (`regex`, `indexmap`, etc.).
- **0** = genuinely hard — would require a real query planner, cost-based optimization, or algorithmic work outside petgraph's primitives.

## Summary

| Axis | Score | Threshold | Verdict |
|---|---|---|---|
| Features (F1–F9) | **9 / 9** | ≥ 7 | PASS |
| Schema (S1–S7) | **7 / 7** | ≥ 6 | PASS |
| **Gate 1** | | | **ADVANCE (as fallback-of-last-resort)** |

**Caveat:** advancing petgraph-baseline is accounting, not ambition. It advances to Gate 3 only as the "none of the others survived" anchor. The 2–3 week build cost means a live off-the-shelf candidate that scores only 60% at Gate 2 is almost always preferable to this.

## Features

### F1 — Fixed-hop label + property match
**Score:** 1
**On-paper test query:**
```rust
// Builder API — no Cypher parser at first; Cypher-subset parser is Phase 2 of the 2-week build
let dup = graph
    .nodes_by_label(Label::Item)
    .tuples()
    .filter(|(a, b)| a.prop("name") == b.prop("name") && a.prop("crate") != b.prop("crate"))
    .collect();
```
**Notes:** `petgraph::visit::IntoNodeReferences` gives O(n) iteration; O(n²) pair scan is acceptable at 15k items (~2M pairs — sub-second in Rust). For larger scales, a hash group-by on `(name)` drops it to O(n). Implementation: half a day.

### F2 — Variable-length path
**Score:** 1
**On-paper test query:**
```rust
let reachable = petgraph::algo::dijkstra(&g, entry_point_node, None, |_| 1);
let within_depth_10 = reachable.into_iter().filter(|(_, d)| *d <= 10).collect();
```
**Notes:** `petgraph::algo::dijkstra` and `bfs` with a depth cutoff solve F2 directly for uniform edge weights. Variable-length path matching with label filters on the edges is a BFS with a filter predicate — one afternoon to implement with visited-set and depth tracking. The openCypher `[:CALLS*1..10]` syntax maps to a `traverse(&g, src, depth=1..=10, edge_label_filter = :CALLS)` helper.

### F3 — Property regex in WHERE
**Score:** 1
**On-paper test query:**
```rust
let re = regex::Regex::new(r"chrono::Utc::now")?;
let hits = g.nodes_by_label(Label::CallSite)
    .filter(|n| re.is_match(n.prop_str("qname")))
    .collect();
```
**Notes:** Add `regex` crate. Trivial. The interpreter maps `WHERE ... =~ /.../` to `regex::Regex::new(pattern).is_match(field)`.

### F4 — OPTIONAL MATCH / left join
**Score:** 1
**On-paper test query:**
```rust
for c in g.nodes_by_label(Label::Concept) {
    let canonical = g.edges_out(c.idx(), EdgeLabel::CanonicalFor)
        .next()
        .and_then(|e| g.node(e.target()));
    // canonical is Option<&Node>; None means OPTIONAL MATCH failed
}
```
**Notes:** Left-join is a for-each-with-Option-look-up. No planner needed. One function in the interpreter.

### F5 — External parameter sets / input bucket joins
**Score:** 1
**On-paper test query:**
```rust
let drops: HashSet<String> = plan_drops.iter().map(|s| s.to_string()).collect();
let hits = g.nodes_by_label(Label::Item)
    .filter(|n| drops.contains(n.prop_str("qname")))
    .collect();
```
**Notes:** Parameters are Rust values passed to the interpreter. The `IN` operator compiles to a `HashSet::contains`. Trivial.

### F6 — NOT EXISTS / anti-join
**Score:** 1
**On-paper test query:**
```rust
let hits = g.nodes_by_label(Label::Item)
    .filter(|i| !g.edges_out(i.idx(), EdgeLabel::Calls)
        .any(|e| g.node(e.target()).label() == Label::Fallback))
    .collect();
```
**Notes:** Anti-join is `!any()`. Map NOT EXISTS → `!inner_match.any(...)`. Trivial.

### F7 — Aggregation + grouping
**Score:** 1
**On-paper test query:**
```rust
let by_crate: HashMap<&str, usize> = g.nodes_by_label(Label::Item)
    .into_grouping_map_by(|n| n.prop_str("crate"))
    .fold(0, |acc, _, _| acc + 1);
```
**Notes:** Use `itertools::Itertools::into_grouping_map_by`. Four or five standard aggregations (COUNT, SUM, MIN, MAX, AVG) is a day of interpreter work. No query planner.

### F8 — Parameterized queries (no string-building)
**Score:** 1
**On-paper test query:**
```rust
// The interpreter IS parameterized by Rust value — there's no string-building surface to attack
let result = cfdb_query!(store, "
    MATCH (i:Item) WHERE i.qname = $qname RETURN i
", qname = user_input)?;
```
**Notes:** We own the parser. Parameters are a first-class AST node — they never interpolate into the query string. This is, in fact, the **easiest** row to get right in a home-built interpreter because we control the pipeline end-to-end. 1.

### F9 — Multi-valued repeated edges
**Score:** 1
**On-paper test query:**
```rust
// `StableDiGraph` allows multi-edges between the same pair natively:
let call_sites: Vec<_> = g.edges_connecting(src, dst)
    .filter(|e| e.weight().label == EdgeLabel::Calls)
    .map(|e| e.weight().prop_str("file_line"))
    .collect();
```
**Notes:** `petgraph::StableDiGraph` (and `Graph` with `edges_connecting`) supports parallel edges natively — each `add_edge` creates a new edge index. Bag semantics are the default; we don't need to fight for them.

## Schema requirements

### S1 — Typed node labels
**Score:** 1
**Evidence:** Labels are a Rust enum (`enum Label { Item, CallSite, Concept, … }`), used as the discriminant on the `Node` type. First-class, exhaustive, pattern-matched at the compiler. petgraph is generic over `N` — we pick `N = Node { label: Label, props: ... }`.

### S2 — Typed edge labels
**Score:** 1
**Evidence:** Same story — `enum EdgeLabel { Calls, TypeOf, ... }`, used as the discriminant on the `Edge` type. petgraph is generic over `E`.

### S3 — Node properties with multiple primitive types
**Score:** 1
**Evidence:** `HashMap<String, PropValue>` where `enum PropValue { Str(String), Int(i64), Float(f64), Bool(bool) }`. Standard pattern. Single morning of code.

### S4 — Edge properties
**Score:** 1
**Evidence:** petgraph's edge weight `E` is arbitrary — make it `struct Edge { label: EdgeLabel, props: HashMap<String, PropValue> }`. Same structure as nodes.

### S5 — Multi-valued repeated edges (bag semantics)
**Score:** 1
**Evidence:** `petgraph::StableDiGraph::add_edge(src, dst, edge)` always creates a new edge, never deduplicates. Bag semantics by default. Set semantics would require extra work — the opposite of the typical problem in this row.

### S6 — Bulk insert
**Score:** 1
**Evidence:** `petgraph::Graph::with_capacity(n_nodes, n_edges)` preallocates. Inserts are O(1) amortized push into the node/edge vectors. 15k nodes + 80k edges is milliseconds — Rust-native, no FFI, no I/O. No "bulk insert API" needed; a for-loop is already the fastest path.

### S7 — Read-only mode / snapshot isolation
**Score:** 1
**Evidence:** Rust's borrow checker *is* the isolation mechanism: `&Graph` is read-only and shares across threads, `&mut Graph` is exclusive. For multi-reader during re-extract, load the fresh graph into a new `Arc<Graph>` and atomic-swap it (`arc_swap` crate or `RwLock<Arc<Graph>>`). The old `Arc` keeps serving readers until their references drop — classic MVCC. This is cleaner than any server-based candidate's isolation story.

## Detailed notes

- **Parser is the real cost.** The 9-row grid is trivially implementable as a builder API in Rust (a few afternoons). The Cypher-subset *parser* is the 2-week cost. Alternatives: (a) skip the parser entirely, expose only the builder API — skills and prescribers compose against the builder, not against a query string; (b) ship a minimal parser for the literal patterns used by cfdb skills (fixed-hop match, variable-length path, regex filter, NOT EXISTS) — ≈ 500 LOC with `chumsky`; (c) adopt an existing Cypher parser crate and plug in our own executor.
- **Determinism is free.** JSONL dump is sorted by `(node_id, edge_id)`; a single-threaded extractor is the default. sha256-stable by construction. RFC §12.1 recipe works verbatim.
- **Scale ceiling.** petgraph is an in-memory crate — the whole graph lives in process RAM. 15k nodes / 80k edges is negligible (under 50 MB). 500k / 5M is still fine. Multi-workspace + enrichment-heavy v0.3 may approach 10M edges; at that point, switching to a proper store becomes a 2026 problem, not a 2025 problem. The `StoreBackend` trait (#3628) is the escape hatch.
- **Loses on**: zero community, zero bug reports, zero third-party audit. Every correctness bug is ours to find. An off-the-shelf candidate with 500+ stars and a CI suite has had more eyes than a solo Rust project will ever get.
- **Wins on**: no FFI, no system deps, no CVEs to track, no upstream archival risk (like Kuzu), 100% debuggable in `rust-gdb`, error messages are our own error messages, and the executor code is 1k LOC we can read. This is not nothing.
- **No "docs" URL**: this candidate's "docs" is "the cfdb-core source" — which does not exist yet. That's the single most important risk: every other candidate has a working docs site *today*. This one's documentation will be written alongside the code.

## Verdict

**ADVANCE (as fallback-of-last-resort).** petgraph-baseline trivially satisfies all 9 features and all 7 schema rows — because we *design* the satisfaction rather than negotiating with an existing system. The catch is the 2–3 week implementation cost and the "1 contributor, 0 stars, 0 external eyes" maintenance posture that no off-the-shelf candidate starts with. This candidate advances to Gate 3 **only** as the anchor against which the other candidates are measured. The pain-point tiebreaker in Gate 3 is precisely the metric that distinguishes "spike took 3 hours of off-the-shelf integration" from "cfdb is now 2 weeks behind schedule building its own executor". If ≥1 live candidate passes Gate 3, petgraph-baseline is **not** the pick; if 0 live candidates pass, this becomes the pick with explicit budget escalation. The methodology's §2 fallback language names this outcome directly, and Gate 3 decides.

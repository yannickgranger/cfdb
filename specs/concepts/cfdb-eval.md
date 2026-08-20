# Spec: cfdb-eval

The Cypher-subset evaluator, split out of `cfdb-petgraph` behind the `GraphReader` port (RFC-057). Depends on `cfdb-core` (the `GraphBackend`/`GraphReader` port pair and the `QueryBackend` trait it implements), `regex` (the `regexp_extract` / `=~` predicate UDFs) and `serde_json` (the set-relationship UDFs parse the `entries_normalized` JSON-array prop) — never on `cfdb-petgraph` or any concrete storage engine, so the evaluator can never reach into a backend's internal graph representation.

`QueryEngine` is the sole `QueryBackend` implementor; storage backends implement `StoreBackend` + `GraphBackend` only. Every graph read the evaluator performs goes through `GraphReader` (`NodeHandle`/`EdgeHandle` handles, ordered scans, adjacency, index-accelerated candidate lookup, ingest diagnostics); the binding table carries handles, never storage indices, and a tripwire test keeps the storage engine's vocabulary out of the evaluator's production sources.

Test suites depend on `cfdb-petgraph` in `[dev-dependencies]` only (a concrete `GraphBackend` to run against, and `PetgraphStore` is the only one that exists) — exempt from the CLEAN-3 dependency-rule gate, which scans `[dependencies]` only. The reverse edge is pinned shut on the storage side too: `cfdb-petgraph`'s manifest may not name `cfdb-eval` in any section.

## QueryEngine

<!-- parent:rfc:cfdb-057-eval-port-split#3.2 anchor:"pub struct QueryEngine<'s, S> { store: &'s S }" -->

Wraps any `GraphBackend` implementor (borrowed shared — evaluation is read-only) and implements `cfdb_core::store::QueryBackend` generically over it. Pure dispatch: resolve the keyspace to its `GraphReader` (the `UnknownKeyspace` guard lives there), run the evaluator, prepend the keyspace's ingest diagnostics to `QueryResult.warnings`. The inherent `execute_explained` returns the same result plus one `ExplainRow` per index-consulted candidate-set resolution (the unknown-label branch returns empty without a row); it stays off the `QueryBackend` trait because it is an evaluator diagnostic, not part of the execution contract.

## ExplainRow

<!-- parent:rfc:cfdb-057-eval-port-split#2.4 anchor:"evaluator observability, not storage observability" -->

One observability row emitted by `QueryEngine::execute_explained` (RFC-035 slice 7 / #186). Carries the rendered `(var:Label)` pattern string and a `hit: ExplainHit` tag naming whether the evaluator's `candidate_nodes` invocation was satisfied through the index fast path or fell back to a full label scan. Stable side-band from `QueryResult` — no explain rows leak into the canonical dump or the keyspace wire format, preserving the RFC-035 §4 determinism invariant. The renderer (`format_line`) is the stable contract consumed by `cfdb scope --explain` dogfood tests.

## ExplainHit

<!-- parent:spec:ExplainRow -->

The closed two-variant enum tagging one `ExplainRow`. `Indexed` means the index fast path fired (`GraphReader::index_candidates` returned a candidate set); `Fallback` means the evaluator used `nodes_with_label` (or `all_nodes_sorted` for label-less patterns). Dogfood tests grep on the arrow-form rendering (`→ indexed` / `→ fallback`) so both variants are load-bearing test primitives for self-dogfood + target-dogfood hit-rate measurements.

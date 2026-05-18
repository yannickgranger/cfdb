// smoke-skip: parameterized ($context) — bound by RFC-036 classifier suite
// classifier-unwired-production.cypher — §A2.1 class 6 (Unwired), production-only variant.
//
// SIBLING: `classifier-unwired.cypher` (the all-kinds default).
// Single point of divergence: this file reads `:Item.reachable_from_production_entry`
// (kind ∈ {test, bench} excluded from BFS seeds); the sibling reads
// `:Item.reachable_from_entry` (all entry kinds). The two files are kept
// structurally identical so any future predicate change to "unwired"
// semantics MUST land in both — solid-architect CRP hygiene
// (RFC §3.3 EDIT 8, MUST language).
//
// fn / method `:Item`s that are NOT reached by ANY production `:EntryPoint`
// (i.e. excluding test/bench entries) and are NOT themselves `:EntryPoint`
// handlers. Surfaces code that compiles, is reached by test or bench code,
// but no production caller — the "test-only-reached production item" class
// that motivates RFC-042 (qbot-core unwired-trading 2057 baseline).
//
// # Inputs
// - `:Item.reachable_from_production_entry` — populated by `enrich_reachability`'s
//   ProductionOnly pass (RFC-042 042-B / issue #392). HIR-dependent
//   (`:EntryPoint.kind` written by `cfdb-hir-extractor`).
// - `:Item.kind` — restricted to fn + method (structs / enums / traits
//   being unreachable is not "unwired logic" — it's "unused type").
// - `:Item.bounded_context` — `enrich_bounded_context`.
// - `:Item.is_test` — filter (compile-scope flag on the item itself, ORTHOGONAL
//   to the entry-kind filter — see `reachable_from_production_entry`'s
//   `SchemaDescribe` row).
//
// # Parameters
// - `$context` — bounded context to scope the finding to.
//
// # Signal vs §A2.1 ideal
// Same as the sibling: BFS reachability is the CHECKED signal; `cargo-udeps` /
// `cargo-machete` cross-validation is NOT checked. ProductionOnly adds one
// degree of specificity over the sibling: instead of "reached by ANY entry",
// it answers "reached by a NON-test, NON-bench entry".
//
// # Guard against entry-point self-match
// Same as the sibling: production entry-point handlers are `EXPOSES`-targets
// of production `:EntryPoint`s, so they self-satisfy
// `reachable_from_production_entry = true` and are filtered out by the
// `= false` predicate. No `NOT EXISTS { (ep)-[:EXPOSES]->(i) }` sub-match
// needed.
//
// # Degradation
// - No HIR extraction → `reachable_from_production_entry` prop absent → empty
//   result. CLI orchestrator warns that reachability enrichment was not run.
// - No `:EntryPoint` nodes → the dual-BFS short-circuits with ran=false; the
//   ProductionOnly pass does not run, and `reachable_from_production_entry`
//   is never written. Same empty-result shape.
// - Catalog has only test/bench entries (no production entries) → every
//   non-handler fn / method has `reachable_from_production_entry = false`
//   and surfaces here. This is the genuine "test-only-reached" signal, NOT
//   a false positive — every production-execution path is empty.
//
// # Determinism
// `ORDER BY qname` — stable row order required by G1.

MATCH (i:Item)
WHERE i.kind IN ['fn', 'method']
  AND i.reachable_from_production_entry = false
  AND i.is_test = false
  AND i.bounded_context = $context
RETURN i.qname AS qname,
       i.name AS name,
       i.kind AS kind,
       i.crate AS crate,
       i.file AS file,
       i.line AS line,
       i.bounded_context AS bounded_context
ORDER BY qname ASC

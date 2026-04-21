// classifier-unwired.cypher — §A2.1 class 6 (Unwired).
//
// fn / method `:Item`s with `reachable_from_entry = false` that are NOT
// themselves `:EntryPoint` handlers. Code that compiles but no user
// action can trigger — either dead (delete) or orphan awaiting wiring.
//
// # Inputs
// - `:Item.reachable_from_entry` — populated by `enrich_reachability`
//   (slice 43-G / issue #110). HIR-dependent.
// - `:Item.kind` — restricted to fn + method (structs / enums / traits
//   being unreachable is not "unwired logic" — it's "unused type").
// - `:Item.bounded_context` — `enrich_bounded_context`.
// - `:Item.is_test` — filter.
//
// # Parameters
// - `$context` — bounded context to scope the finding to.
//
// # Signal vs §A2.1 ideal
// The addendum §A2.1 class 6 signals are:
//   - BFS reachability from `:EntryPoint` = empty (CHECKED)
//   - `cargo-udeps` / `cargo-machete` cross-validation (NOT checked —
//     requires the extractor to shell out at enrichment time, which
//     is out of scope for the petgraph enrichment layer; those tools
//     also answer a dep-graph question, not a call-graph question)
//
// # Guard against entry-point self-match
// Entry-point handlers are items with an `EXPOSES` edge from a
// `:EntryPoint`. They ARE reachable from themselves by construction,
// but a single-hop self-reachability of a handler is what populates
// `reachable_from_entry = true` on the handler in the first place.
// The reachability enrichment's BFS includes the seed node, so the
// `reachable_from_entry = false` filter already excludes handlers.
//
// No explicit `NOT EXISTS { (ep)-[:EXPOSES]->(i) }` sub-match is
// therefore needed — the reachability attr subsumes it.
//
// # Degradation
// - No HIR extraction → `reachable_from_entry` prop absent → empty
//   result. CLI orchestrator warns that reachability enrichment was
//   not run.
// - No `:EntryPoint` nodes (e.g. a pure library crate keyspace) →
//   every fn / method has `reachable_from_entry = false` → every
//   non-test fn surfaces as "unwired". This is correct for a library
//   but noisy for the scope verb — the CLI orchestrator tags this
//   shape explicitly in the warnings.
//
// # Determinism
// `ORDER BY qname` — stable row order required by G1.

MATCH (i:Item)
WHERE i.kind IN ['fn', 'method']
  AND i.reachable_from_entry = false
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

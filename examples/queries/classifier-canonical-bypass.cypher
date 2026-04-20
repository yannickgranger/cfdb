// classifier-canonical-bypass.cypher — §A2.1 class 5 (CanonicalBypass).
//
// Surfaces canonical-side orphans: `:Item`s declared `CANONICAL_FOR`
// some `:Concept` that no `:EntryPoint` reaches. A canonical impl with
// zero reachable callers is (by construction) being bypassed — either
// because callers resolve via a non-canonical wire form, or because the
// canonical side has no callers at all. Both shapes route to "rewire OR
// delete bypass".
//
// # Inputs
// - `:Concept` nodes + `(:Item)-[:CANONICAL_FOR]->(:Concept)` edges —
//   populated by `enrich_concepts` (slice 43-F / issue #109). Requires
//   `.cfdb/concepts/*.toml` declarations.
// - `:Item.reachable_from_entry` — populated by `enrich_reachability`
//   (slice 43-G / issue #110). HIR-dependent.
// - `:Item.bounded_context` — `enrich_bounded_context`.
//
// # Parameters
// - `$context` — bounded context to scope the finding to. Only emits
//   canonical items whose `bounded_context = $context`. Bypass callers
//   in a different context that target this context's canonical impl
//   are that other context's findings (by the DDD rule — a finding
//   belongs to the context that owns the offending item).
//
// # Output shape
// Projects `Finding`-compatible columns for the unreachable canonical
// item. Mirrors the `canonical-unreachable.cypher` projection minus
// the concept-name column (which is evidence, not a structural column).
//
// # Signal vs the three-verdict Pattern C
// Pattern C ships three verdicts:
//   - CANONICAL_CALLER       — healthy wiring (NOT a finding)
//   - BYPASS_REACHABLE       — a live wiring bug (IS a finding, but
//     requires per-concept `$bypass_callee_name` / `$caller_regex`
//     parameters the generic classifier cannot supply)
//   - BYPASS_DEAD            — dead bypass (IS a finding, same param
//     requirement as BYPASS_REACHABLE)
// Plus CANONICAL_UNREACHABLE which is the v0.1 generic signal used here.
//
// The classifier's v0.1 form surfaces CANONICAL_UNREACHABLE because:
//   1. It is parameterless (no `$bypass_callee_name` / `$caller_regex`
//      needed — the bypass is implicit in "canonical has no reachable
//      callers").
//   2. It directly evidences the class 5 fix strategy ("rewire or
//      delete bypass") — either callers need to be wired through the
//      canonical, or the canonical is dead and the concept declaration
//      should be retracted.
//
// Per-concept BYPASS_REACHABLE / BYPASS_DEAD rules remain as
// `examples/queries/canonical-bypass-{reachable,dead}.cypher` for
// concept-specific triage runs. The CLI orchestrator mentions this
// scope-limitation in the class 5 warning.
//
// # Degradation
// - No `.cfdb/concepts/*.toml` → no `CANONICAL_FOR` edges → empty result.
//   CLI orchestrator warns that concepts enrichment was not run.
// - No HIR extraction → `reachable_from_entry` prop absent → empty
//   result (prop equality on absent prop returns false). CLI orchestrator
//   warns that reachability enrichment was not run.
//
// # Determinism
// `ORDER BY qname` — stable row order required by G1.

MATCH (i:Item)-[:CANONICAL_FOR]->(c:Concept)
WHERE i.reachable_from_entry = false
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

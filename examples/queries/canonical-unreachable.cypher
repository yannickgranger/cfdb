// smoke-skip: parameterized ($concept) — bound by RFC-cfdb §A1.1 ladder driver
// canonical-unreachable.cypher — Pattern C (RFC-029 v0.2 §A1.4),
// verdict: CANONICAL_UNREACHABLE.
//
// Finds every `:Item` declared canonical for a concept (via
// `(:Item)-[:CANONICAL_FOR]->(:Concept)` materialized by
// `enrich_concepts`) that no `:EntryPoint` reaches. Two distinct
// degraded shapes produce rows here:
//
//  - The canonical impl exists but NO CALLERS in the whole keyspace call
//    it. Symptom of an incomplete migration — callers still bypass.
//  - The canonical impl has callers, but none of THOSE callers are
//    themselves reachable from an entry point. The canonical side is
//    orphaned behind a chain of dead wrappers.
//
// This query conflates both shapes deliberately: the remediation is the
// same in both cases ("wire bypass callers through the canonical OR
// delete the canonical"), and the distinction between "zero callers"
// vs "dead callers" is cheap to produce with a follow-up `list-callers`
// query on any row returned here.
//
// Parameters:
//   $concept — concept name to interrogate. The CANONICAL_FOR edge is
//              MATCHed — so this rule only emits rows when a canonical
//              declaration actually exists. Running this rule on a
//              concept with no CANONICAL_FOR edges returns empty, which
//              is the correct "no declaration, nothing to check" answer.
//
// Reachability source: `:Item.reachable_from_entry` (bool) populated by
// `enrich_reachability` (slice 43-G / issue #110). Callers MUST run
// `enrich-reachability` first; absent the attr, the predicate filters
// all rows out and this rule silently returns empty.
//
// Filters: `can.is_test = false` — test-only canonical impls would be
// false positives (test fixtures never need an entry point).
//
// Usage:
//   cfdb extract --workspace <dir> --db <db> --keyspace <ks> --features hir
//   cfdb enrich-concepts     --db <db> --keyspace <ks> --workspace <dir>
//   cfdb enrich-reachability --db <db> --keyspace <ks>
//   cfdb violations --db <db> --keyspace <ks> \
//       --params '{"concept":"ledger"}' \
//       --rule canonical-unreachable.cypher

MATCH (can:Item)-[:CANONICAL_FOR]->(c:Concept)
WHERE c.name = $concept
  AND can.reachable_from_entry = false
  AND can.is_test = false
RETURN c.name AS concept,
       can.qname AS canonical_item,
       '(no call site — canonical impl is unreachable)' AS call_site,
       can.qname AS caller,
       'CANONICAL_UNREACHABLE' AS verdict,
       can.file AS evidence
ORDER BY canonical_item ASC

// raid-missing-canonical.cypher — RFC-036 §3.5 raid template 4/5.
//
// Detects concepts named in `$rewrite` for which NO `:Item` carries a
// `:CANONICAL_FOR` edge. A rewrite-bucket concept without a canonical
// implementation target in the graph is an unresolved TODO: the
// author committed to providing a new canonical, but the code side
// doesn't have one yet. Merging the raid at this state ships a
// dangling reference.
//
// # v2 — OPTIONAL MATCH + collect + size(=0) pattern
//
// The cfdb-query v0.3 `NOT EXISTS { MATCH (canonical)-[:CANONICAL_FOR]->(c) }`
// subquery evaluates as a standalone query — outer-scope variables do
// not propagate into the inner MATCH. The inner `(c)` would therefore
// bind to an unrelated free variable, not the outer-matched concept.
//
// Workaround: `OPTIONAL MATCH` the canonical, aggregate collected
// canonicals per concept into a list, then filter to concepts whose
// list has size 0. The aggregation binds the outer concept through
// `WITH` rather than through subquery scope.
//
// # Parameters
//
//   $rewrite: List(String) — concept names requesting new canonicals
//
// # Output
//
//   concept_name — the rewrite-bucket concept with no canonical
//
// Authors triage: either (a) ship the canonical before merge, (b)
// demote the concept from `rewrite` to `portage` / `drop` as
// appropriate, or (c) document in KNOWN_GAPS.md why the canonical is
// deferred.

MATCH (c:Concept)
OPTIONAL MATCH (canonical:Item)-[:CANONICAL_FOR]->(c)
WITH c.name AS concept_name,
     collect(canonical) AS canonicals
WHERE concept_name IN $rewrite
  AND size(canonicals) = 0
RETURN concept_name
ORDER BY concept_name ASC

// Params: $context_a (context:<name>), $context_b (context:<name>)
// Returns: (qname, line, reason) — canonical three-column violation format.
//
// Detect a :Crate whose `.name` appears in the crate-set of BOTH contexts —
// a candidate DDD homonym flagged for manual bounded-context review.
//
// Uses only top-level MATCH + AND composition on IN predicates — no positive
// EXISTS, no inner-subquery WHERE extension, no non-existent edge labels.
// RFC-034 R1 C2 seed (reframed from the deferred re-export shape).
MATCH (c:Crate)
WHERE c.name IN $context_a
  AND c.name IN $context_b
RETURN c.name AS qname, 0 AS line, 'crate is a member of both contexts — candidate DDD homonym' AS reason
ORDER BY qname

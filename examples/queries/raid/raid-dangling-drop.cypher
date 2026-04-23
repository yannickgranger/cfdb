// raid-dangling-drop.cypher — RFC-036 §3.5 raid template 2/5.
//
// Detects items in `$drop` that still have INCOMING `CALLS` edges from
// the `$portage` or `$glue` buckets. If we drop X but a portaged or
// glue-rewritten item still calls X, the raid will leave the moved /
// rewritten item pointing at a deleted symbol — the dangling-drop
// failure mode.
//
// # v2 scope — CALLS edges only
//
// The RFC-036 §3.5 shape covers both `CALLS` and `TYPE_OF` incoming
// edges. The v0.3 cfdb-query subset does not support edge-label
// alternation (`[:CALLS|TYPE_OF]`) or UNION — a TYPE_OF variant would
// require a sibling template file. v2 ships CALLS-only; TYPE_OF is a
// v2.1 extension point.
//
// # Parameters
//
//   $portage: List(String) — item qnames moved as-is
//   $glue:    List(String) — adapter/wiring items being rewritten
//   $drop:    List(String) — item qnames discarded
//
// # Output
//
//   dropped_qname  — the dropped item that still has callers
//   caller_qname   — the surviving caller (portage / glue)
//   caller_bucket  — "portage" or "glue" (INFORMATIONAL — distinguishable
//                    caller-side; the query itself returns both together)
//
// (In v2 the query does not emit a `caller_bucket` literal — the
// subset lacks case-style expressions. Callers partition the result by
// checking `caller_qname` against each bucket after the query.)

MATCH (caller:Item)-[:CALLS]->(dropped:Item)
WHERE dropped.qname IN $drop
  AND (caller.qname IN $portage OR caller.qname IN $glue)
RETURN dropped.qname AS dropped_qname,
       caller.qname AS caller_qname
ORDER BY dropped_qname ASC, caller_qname ASC

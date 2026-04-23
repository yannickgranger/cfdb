// raid-hidden-callers.cypher — RFC-036 §3.5 raid template 3/5.
//
// Detects items in `$portage` that have INCOMING `CALLS` edges from
// OUTSIDE the source_context. These are "hidden callers" — code the
// author didn't consider when drafting the plan, because their view
// was crate-local. Moving the portaged item without coordinating
// these external callers will break the build.
//
// # Parameters
//
//   $source_context: Scalar(String) — crate name being raided
//   $portage:        List(String)   — item qnames moved as-is
//
// # Output
//
//   portaged_qname  — the moved item that has external callers
//   external_qname  — the caller outside source_context
//   external_crate  — crate of the external caller (for routing)
//
// Authors triage: either coordinate the caller's migration (if it
// belongs to the raided domain) or lift the portaged item to the
// shared kernel (if the caller is architecturally independent).

MATCH (external:Item)-[:CALLS]->(portaged:Item)
WHERE portaged.qname IN $portage
  AND external.crate <> $source_context
RETURN portaged.qname AS portaged_qname,
       external.qname AS external_qname,
       external.crate AS external_crate
ORDER BY portaged_qname ASC, external_qname ASC

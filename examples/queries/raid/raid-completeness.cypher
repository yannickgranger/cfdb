// raid-completeness.cypher — RFC-036 §3.5 raid template 1/5.
//
// Detects items in the source_context that are NOT explicitly named in
// any raid-plan bucket. Completeness of the plan = the author accounts
// for every item in the crate being raided. An omitted item is either
// (a) a real oversight — the author forgot to decide — or (b) an
// intentional skip the author should document in KNOWN_GAPS.md rather
// than leave silent.
//
// # Parameters (bound by the consumer workspace, RFC-036 CP7)
//
//   $source_context: Scalar(String) — crate name being raided
//   $portage:        List(String)   — item qnames moved as-is
//   $rewrite:        List(String)   — concept names getting new canonicals
//   $glue:           List(String)   — adapter/wiring items being rewritten
//   $drop:           List(String)   — item qnames discarded
//
// # v2 scope — qname buckets only
//
// `$rewrite` contains CONCEPT names (not item qnames) per RFC-036
// §3.5. The cfdb-query v0.3 subset does NOT let a `NOT EXISTS`
// subquery run an `IN` predicate against an outer list parameter
// (the subquery's WHERE only supports scalar comparisons — see
// `cfdb-query::parser::predicate::subquery_parser`). v2 therefore
// checks membership in the three qname buckets only
// (`$portage` / `$glue` / `$drop`).
//
// Items that are canonicals for a rewrite concept but not in any
// qname bucket WILL flag as completeness findings. The raid-plan
// author triages: either add the canonical item to `$portage` (the
// common case) or document the intentional omission in
// KNOWN_GAPS.md. This is load-bearing for plan hygiene — every item
// needs an explicit placement decision.
//
// v2.1 extension point: lift the `NOT EXISTS { ... WHERE IN $rewrite }`
// path once the subquery WHERE grammar accepts list predicates.
//
// # Output
//
//   qname, kind, name — identity of the unclaimed item.

MATCH (i:Item)
WHERE i.crate = $source_context
  AND NOT i.qname IN $portage
  AND NOT i.qname IN $glue
  AND NOT i.qname IN $drop
RETURN i.qname AS qname,
       i.kind AS kind,
       i.name AS name
ORDER BY qname ASC

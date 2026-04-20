// canonical-bypass-dead.cypher — Pattern C (RFC-029 v0.2 §A1.4),
// verdict: BYPASS_DEAD.
//
// Finds every prod call site that resolves the concept via the
// non-canonical wire-level form AND is NOT reachable from any
// `:EntryPoint`. These rows are **dead code that would be a wiring bug
// if anyone reached them** — no user action can trigger them today.
// Action: delete.
//
// Parameters: identical to canonical-bypass-reachable.cypher.
//
//   $concept             — concept name, returned for provenance.
//   $bypass_callee_name  — the forbidden method name at the wire level.
//   $caller_regex        — regex over `caller.qname` scoping the resolvers.
//
// Reachability source: `:Item.reachable_from_entry` (bool) populated by
// `enrich_reachability` (slice 43-G / issue #110). Unlike the
// `BYPASS_REACHABLE` sibling, this rule relies on `reachable_from_entry =
// false` being a trustworthy signal — which it only is once
// `enrich_reachability` has actually run and populated the attr on every
// `:Item`. If reachability enrichment was skipped, every row returned
// here is a false positive (the prop is absent → the equality predicate
// fails → an empty result set would be returned instead, so the degraded
// behavior here is "silent no-op" not "false positives"; the sibling
// reachable rule's degraded behavior is also silent empty).
//
// Filters: `is_test = false` on both the caller and the callsite.
//
// Usage:
//   cfdb extract --workspace <dir> --db <db> --keyspace <ks> --features hir
//   cfdb enrich-concepts     --db <db> --keyspace <ks> --workspace <dir>
//   cfdb enrich-reachability --db <db> --keyspace <ks>
//   cfdb violations --db <db> --keyspace <ks> \
//       --params '{"concept":"ledger","bypass_callee_name":"append","caller_regex":".*::LedgerService::.*"}' \
//       --rule canonical-bypass-dead.cypher

MATCH (caller:Item)-[:INVOKES_AT]->(cs:CallSite)
WHERE caller.qname =~ $caller_regex
  AND cs.callee_last_segment = $bypass_callee_name
  AND caller.is_test = false
  AND cs.is_test = false
  AND caller.reachable_from_entry = false
RETURN $concept AS concept,
       cs.callee_path AS call_site,
       caller.qname AS caller,
       'BYPASS_DEAD' AS verdict,
       cs.file AS evidence
ORDER BY caller ASC

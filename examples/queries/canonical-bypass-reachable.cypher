// canonical-bypass-reachable.cypher — Pattern C (RFC-029 v0.2 §A1.4),
// verdict: BYPASS_REACHABLE.
//
// Finds every prod call site that resolves the concept via the
// non-canonical wire-level form AND is reachable from at least one
// `:EntryPoint`. These rows are the **live wiring bugs** — a user action
// can trigger them. Action: rewire.
//
// Parameters: identical to canonical-bypass-caller.cypher, except
// `$canonical_callee_name` is replaced by `$bypass_callee_name` — the
// non-canonical form (e.g. "append" vs the canonical "append_idempotent").
//
//   $concept             — concept name, returned for provenance.
//   $bypass_callee_name  — the forbidden method name at the wire level.
//   $caller_regex        — regex over `caller.qname` scoping the resolvers.
//
// Reachability source: `:Item.reachable_from_entry` (bool) populated by
// `enrich_reachability` (slice 43-G / issue #110). Callers MUST run
// `cfdb enrich-reachability` before this rule; without that enrichment
// the `reachable_from_entry` prop is absent and the WHERE clause filters
// all rows out. The degraded-path report from `enrich_reachability` warns
// when `:EntryPoint` nodes are missing — the rule surface has no way to
// distinguish "bypass exists but is dead" from "reachability not run".
//
// Filters: `is_test = false` on both the caller and the callsite.
//
// Usage:
//   cfdb extract --workspace <dir> --db <db> --keyspace <ks> --features hir
//   cfdb enrich-concepts     --db <db> --keyspace <ks> --workspace <dir>
//   cfdb enrich-reachability --db <db> --keyspace <ks>
//   cfdb violations --db <db> --keyspace <ks> \
//       --params '{"concept":"ledger","bypass_callee_name":"append","caller_regex":".*::LedgerService::.*"}' \
//       --rule canonical-bypass-reachable.cypher

MATCH (caller:Item)-[:INVOKES_AT]->(cs:CallSite)
WHERE caller.qname =~ $caller_regex
  AND cs.callee_last_segment = $bypass_callee_name
  AND caller.is_test = false
  AND cs.is_test = false
  AND caller.reachable_from_entry = true
RETURN $concept AS concept,
       cs.callee_path AS call_site,
       caller.qname AS caller,
       'BYPASS_REACHABLE' AS verdict,
       cs.file AS evidence
ORDER BY caller ASC

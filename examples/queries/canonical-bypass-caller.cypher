// canonical-bypass-caller.cypher — Pattern C (RFC-029 v0.2 §A1.4), verdict:
// CANONICAL_CALLER.
//
// Given a declared canonical concept (a `:Concept` materialized by
// `enrich_concepts` with at least one `(:Item)-[:CANONICAL_FOR]->(:Concept)`
// edge), find every prod call site that resolves that concept VIA the
// canonical wire-level method. These rows are the "OK" signal — they are
// the shape the bypass queries contrast against.
//
// Parameters:
//   $concept               — the concept name (e.g. "ledger"). Returned in
//                            the output for row-provenance; not joined in
//                            the MATCH because the CANONICAL_FOR edge is
//                            not per-callsite (it's crate-wide per the
//                            concepts TOML declaration — see
//                            `enrich_concepts` emission rules). The concept
//                            link is enforced by the sibling
//                            canonical-unreachable.cypher rule, which DOES
//                            MATCH the `:Concept` + CANONICAL_FOR edges.
//   $canonical_callee_name — the canonical method name at the wire level
//                            (e.g. "append_idempotent"). Matched against
//                            `cs.callee_last_segment`.
//   $caller_regex          — regex over `caller.qname` scoping which items
//                            count as "resolvers of this concept". E.g.
//                            `.*::LedgerService::.*` scopes to
//                            `LedgerService`. Concept-resolving callers
//                            can live in any crate that calls into the
//                            canonical impl, so the CANONICAL_FOR crate
//                            membership alone is not sufficient — a
//                            caller-side filter is needed.
//
// Why `$canonical_callee_name` is a separate param from the concept name:
// the concept declaration lives in `.cfdb/concepts/<concept>.toml`,
// loaded by `cfdb-concepts`, materialized by
// `PetgraphStore::enrich_concepts` (slice 43-F / issue #109). The TOML
// declares WHICH CRATE is canonical; it does NOT declare WHICH METHOD
// NAME on that crate is the canonical entry point. A crate can expose
// both `append` and `append_idempotent`; only the latter is the canonical
// wire-level form. Until concepts TOML grows a `canonical_method_patterns`
// field, this parameter is explicit.
//
// Output: one row per CANONICAL_CALLER call site. Non-empty result is the
// "healthy wiring" evidence — zero rows on a tree that DOES resolve the
// concept is itself a signal (see canonical-unreachable.cypher).
//
// Filters: `is_test = false` on both the caller and the callsite — tests
// legitimately exercise any wire-level form to build scenarios, including
// the "canonical" side, and test-only canonical-caller rows are noise.
//
// Usage:
//   cfdb extract --workspace <dir> --db <db> --keyspace <ks> --features hir
//   cfdb enrich-concepts --db <db> --keyspace <ks> --workspace <dir>
//   cfdb query --db <db> --keyspace <ks> \
//       --params '{"concept":"ledger","canonical_callee_name":"append_idempotent","caller_regex":".*::LedgerService::.*"}' \
//       "$(cat canonical-bypass-caller.cypher)"

MATCH (caller:Item)-[:INVOKES_AT]->(cs:CallSite)
WHERE caller.qname =~ $caller_regex
  AND cs.callee_last_segment = $canonical_callee_name
  AND caller.is_test = false
  AND cs.is_test = false
RETURN $concept AS concept,
       cs.callee_path AS call_site,
       caller.qname AS caller,
       'CANONICAL_CALLER' AS verdict,
       cs.file AS evidence
ORDER BY caller ASC

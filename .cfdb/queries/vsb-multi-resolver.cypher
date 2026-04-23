// vsb-multi-resolver.cypher — RFC-036 §3.2 VSB detector.
//
// Detects Vertical Split-Brain by the `multi-resolver-for-one-registered-param`
// shape: a single `:EntryPoint` registers a `:Param` of type `T`, and
// traversing the call graph from that entry point reaches two or more
// distinct `:Item{kind:"fn"}` nodes whose `RETURNS` edge points to the
// same `:Item` whose `qname` matches `p.type_normalized`.
//
// When that happens the system has two parallel resolution paths for the
// same external-surface parameter — the canonical VSB failure mode
// (RFC-036 §3.2 / issue #202 scars: param-effect-canary, MCP boundary
// fix AC template, compound-stop layer isolation).
//
// # Reconstruction from the RFC-036 reference shape
//
// RFC-036 §3.2 gives:
//
//     MATCH (e:EntryPoint)-[:EXPOSES]->(h:Item)
//     MATCH (e)-[:REGISTERS_PARAM]->(p:Param)
//     MATCH (h)-[:CALLS*1..10]->(f:Item {kind: 'fn'})
//     WHERE (f)-[:RETURNS]->(t:Item) AND t.qname = p.type_normalized
//     WITH e, p, collect(DISTINCT f.qname) AS resolvers
//     WHERE size(resolvers) > 1
//     RETURN e.name, p.name, resolvers
//
// The cfdb-query Cypher subset does not support bare pattern predicates
// inside `WHERE`; EXISTS-style pattern filters only appear via
// `NOT EXISTS { MATCH ... }`. The reference is therefore expanded into a
// fourth `MATCH` clause whose implicit-AND join against the first three
// is semantically identical: every returned `(e, p, f)` binding also
// binds a `t` such that `f -[:RETURNS]-> t`. The subsequent `t.qname =
// p.type_normalized` comparison then lives in the top-level WHERE.
//
// # Output columns
//
//   entry_point   — display name of the :EntryPoint (CLI verb / HTTP
//                   path / MCP tool / cron schedule)
//   entry_qname   — handler qname, disambiguates two entry points that
//                   share the same display name
//   param_name    — :Param.name registered on the entry point
//   param_type    — :Param.type_normalized, equals the shared
//                   `RETURNS` target qname
//   resolvers     — lex-sorted list of distinct fn qnames reachable
//                   from the entry point that return this type
//
// # Usage
//
//   cfdb query --db <dir> --keyspace <ks> "$(cat .cfdb/queries/vsb-multi-resolver.cypher)"
//
// Expected: empty on a clean tree. Any row is a VSB candidate and
// should be routed per the RFC-036 §A2.3 skill-routing rules:
//   - Same bounded context → /sweep-epic (consolidate resolvers)
//   - Cross-context         → /operate-module (context-mapping)
//
// # Scope and known gaps (v0.3.1)
//
// - BFS bound `*1..10` — deeper chains truncate. Matches RFC-036's
//   written bound and the pattern_b vertical-split-brain convention.
// - `is_test = false` filter on `f` excludes test-scoped resolvers so
//   scar-corpus assertions don't mask production-path regressions.
// - Entry-point-scoped: two resolvers reachable from DIFFERENT entry
//   points are two distinct paths, not a fork — the per-EP join
//   excludes that shape by construction.

MATCH (e:EntryPoint)-[:EXPOSES]->(h:Item)
MATCH (e)-[:REGISTERS_PARAM]->(p:Param)
MATCH (h)-[:CALLS*1..10]->(f:Item {kind: 'fn'})
MATCH (f)-[:RETURNS]->(t:Item)
WHERE t.qname = p.type_normalized
  AND f.is_test = false
WITH e.name AS entry_point,
     e.handler_qname AS entry_qname,
     p.name AS param_name,
     p.type_normalized AS param_type,
     collect(DISTINCT f.qname) AS resolvers
WHERE size(resolvers) > 1
RETURN entry_point,
       entry_qname,
       param_name,
       param_type,
       resolvers
ORDER BY entry_point ASC, param_name ASC

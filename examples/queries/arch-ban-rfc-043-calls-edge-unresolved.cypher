// arch-ban-rfc-043-calls-edge-unresolved.cypher
//   — RFC-044 §3.8 slice 044-H (issue #427); encodes the :CALLS edge
//     resolved-discriminator contract (RFC-029 §A1.2 / RFC-043 §4).
//
// # Invariant encoded (:CALLS edge `resolved` discriminator)
//
// The `:CALLS` edge carries a `resolved` bool discriminator
// (`cfdb schema-describe`: "`true` when the dispatch was resolved via HIR
// type inference; `false` for textual / unresolved baseline.
// SchemaVersion v0.1.4+ only. The HIR-based extractor is the FIRST producer
// of :CALLS edges — v0.1.3 and earlier graphs have no CALLS edges at all").
//
// Because ONLY `cfdb-hir-extractor` produces `:CALLS` edges, and it emits a
// `:CALLS` edge ONLY when it actually resolved the call dispatch via HIR,
// the invariant is:
//
//     every :CALLS edge has resolved = true
//
// A `:CALLS` edge with `resolved = false` is impossible under the
// contract — it would mean an unresolved dispatch produced a resolved-call
// edge, i.e. the syn extractor (which never emits :CALLS) leaked an edge,
// or the hir extractor emitted a :CALLS for a dispatch it failed to
// resolve. Either is a producer bug. RFC-043 §4 holds the HIR extractor to
// this; before this rule it was reviewer-only.
//
// # Inversion (positive invariant → its negation)
//
// Positive invariant: every :CALLS edge is `resolved = true`. Negation:
// match the edge and select those with `r.resolved = false`. A clean tree
// yields none.
//
// # Why an edge-attribute filter, not a node filter
//
// `resolved` is an EDGE property on `:CALLS`, so the rule binds the edge
// (`-[r:CALLS]->`) and filters `r.resolved`. The evaluator supports
// edge-attribute access in WHERE (`crates/cfdb-eval/src/eval/predicate.rs`
// `Binding::EdgeRef`). Endpoints are surfaced via the edge's `src`/`dst`
// node-id pseudo-properties for triage.
//
// # File location — documented deviation from RFC §3.8
//
// Ships in `examples/queries/` (not `.cfdb/queries/`) so the existing
// `examples/queries/arch-ban-*.cypher` globs in `.gitea/workflows/ci.yml`
// (~line 201) and `ci/cross-dogfood.sh` (~line 86) auto-enforce it with
// zero CI-workflow edits.
//
// # Usage
//   cfdb violations --db <dir> --keyspace <ks> \
//     --rule examples/queries/arch-ban-rfc-043-calls-edge-unresolved.cypher
//
// Expected: empty on a clean tree. Any row is a :CALLS edge carrying
// `resolved = false`, which violates the producer contract.

MATCH (caller:Item)-[r:CALLS]->(callee:Item)
WHERE r.resolved = false
RETURN caller.qname AS caller_qname,
       callee.qname AS callee_qname
ORDER BY caller_qname ASC, callee_qname ASC

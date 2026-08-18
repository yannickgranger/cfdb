// matchsite-top-matched-on-enums.cypher — RFC-053 §3.2 / slice 53-B.
//
// "Which workspace enums are dispatched on the most, and from how many
// distinct `match` sites?" — a discovery / survey query over the
// MATCHES_ON resolution facts, NOT a ban rule and NOT a gate. It returns
// structured facts an agent (or a maintainer choosing a fence target for
// slice 53-C) can reason about without re-reading Rust source.
//
// # Shape
//
// Every resolved `(:MatchSite)-[:MATCHES_ON]->(:Item{kind:"enum"})` edge
// links a `match` dispatch site to the workspace enum its name-level
// `matched_path` prefix resolved to (RFC-053 §3.2). Grouping by the enum
// and counting incoming edges yields the enum's dispatch in-degree — a
// SURVEY metric, not a violation signal (§3.6: matching on your own enum
// from many fns is normal Rust). A high count simply flags an enum whose
// dispatch-to-values is spread across many sites — a candidate to
// designate a canonical dispatch owner for (the 53-C fence templates).
//
// # Why the count is materialised in WITH
//
// The v0.1 Cypher subset does not carry node references (`e`) across a
// WITH boundary — only scalar `Binding::Value`s
// (`crates/cfdb-eval/src/eval/with_clause.rs`). So the enum's
// identity columns (`qname`, `crate`) are materialised as scalars in the
// WITH clause alongside `COUNT(*)`, mirroring the
// `arch-ban-rfc-042-duplicate-entrypoint.cypher` group-by-count shape;
// RETURN then just renames each scalar.
//
// # Notes
//
// - Only RESOLVED sites appear — an external-type match (e.g.
//   `syn::Visibility`) has no MATCHES_ON edge and is invisible here by
//   design (§3.2, the honest name-level-only representation). The raw
//   `:MatchSite` population, resolved or not, is surveyed separately by a
//   query over `:MatchSite` nodes directly.
// - `e.kind = 'enum'` is redundant with the edge's own grammar (MATCHES_ON
//   only ever targets enums) but is kept explicit so the query documents
//   the invariant it relies on.
// - Test-mode dispatch sites are counted. To scope to production only,
//   add `AND m.is_test = false` to the MATCH filter.
//
// # Determinism
//
// `ORDER BY match_site_count DESC, enum_qname ASC` gives a stable "top N"
// ordering across runs; the trailing `enum_qname` tiebreak makes ties
// byte-stable. `LIMIT 20` keeps the survey to the head of the
// distribution.
//
// # Usage
//
//   cfdb query --db <dir> --keyspace <ks> \
//       "$(cat examples/queries/matchsite-top-matched-on-enums.cypher)"
//
// Expected: zero rows on a tree with no resolved dispatch (e.g. a crate
// that only matches external types); otherwise the most-dispatched
// workspace enums, highest in-degree first.

MATCH (m:MatchSite)-[:MATCHES_ON]->(e:Item)
WHERE e.kind = 'enum'
WITH e.qname AS enum_qname,
     e.crate AS enum_crate,
     COUNT(*) AS match_site_count
RETURN enum_qname AS enum_qname,
       enum_crate AS enum_crate,
       match_site_count AS match_site_count
ORDER BY match_site_count DESC, enum_qname ASC
LIMIT 20

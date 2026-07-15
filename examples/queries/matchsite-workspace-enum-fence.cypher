// smoke-skip: parameterized ($enum, $canonical) — a fence TEMPLATE; each repo instantiates it as a concrete arch-ban-*.cypher
// matchsite-workspace-enum-fence.cypher — RFC-053 §3.5 / §3.6, slice 53-C.
//
// Split-resolution-point fence TEMPLATE, workspace-enum form. A repo copies
// this shape into a concrete `examples/queries/arch-ban-<repo>-<enum>.cypher`
// rule that pins `$enum` and `$canonical` to literals. It is NOT itself a
// gate: parameterized ⇒ smoke-skipped, never enforced.
//
// # When to use this form (vs. the external-type form)
//
// Use the WORKSPACE-enum form when the fenced type is a workspace `:Item`
// enum — one that resolution can reach. Its dispatch sites carry a resolved
// `(:MatchSite)-[:MATCHES_ON]->(:Item{kind:"enum"})` edge (RFC-053 §3.2).
// Anchoring on that edge is HOMONYM-PROOF: the edge points at the exact
// workspace qname, so an unrelated external type of the same name (which has
// no such edge — external types resolve to nothing) is never caught. This is
// the tool to reach for whenever the name is shared or arms are written
// unqualified. For a type with no workspace node (a dependency's type), the
// resolved edge never exists — use `matchsite-external-type-fence.cypher`.
//
// # Fence, not survey — designate a canonical owner
//
// RFC-053 §3.6: raw MATCHES_ON in-degree is a SURVEY metric, not a ban
// signal — matching your own enum from many fns is normal Rust
// (`matchsite-top-matched-on-enums.cypher` is the survey). A FENCE requires
// a designation: "type T's dispatch-to-values is owned by module/fn F",
// expressed as the single canonical-site NOT-clause below.
//
// # Guardrail (RFC-053 §3.6, verbatim from ratification)
//
// One fence file per fenced type; at most ONE canonical-site NOT-clause per
// file; never an accreting exception list — a file that grows NOT-clauses is
// a de facto allowlist, rejected on sight (project CLAUDE.md §3). See
// `docs/split-resolution-fences.md`.
//
// # Parameters
//   $enum      — Rust regex over the target enum's `qname`, anchored, e.g.
//                '^cfdb_core::visibility::Visibility$'
//   $canonical — Rust regex over the workspace-relative `file`; the single
//                designated owner to exempt.
//
// # Usage
//   cfdb query --db <dir> --keyspace <ks> \
//     --params '{"enum":"^my_crate::phase::Phase$","canonical":".*crates/fsm/src/dispatch.*"}' \
//     "$(cat matchsite-workspace-enum-fence.cypher)"
//
// Expected: empty when every dispatch on the enum lives at the canonical
// site. Any row is a resolved dispatch on the designated enum outside its
// owner — a split resolution point.

//
// NOTE on `$canonical` / file regexes: node `file` props are ABSOLUTE
// paths in practice (the extractor's workspace-relative strip silently
// falls back to the absolute path whenever `--workspace` is not an
// exact prefix — e.g. CI's `--workspace .`). Always wrap file patterns
// as `.*<path fragment>.*`; an `^`-anchored relative path never matches
// and produces a silently-dead fence.
MATCH (m:MatchSite)-[:MATCHES_ON]->(t:Item)
WHERE t.kind = 'enum'
  AND t.qname =~ $enum
  AND m.is_test = false
  AND NOT m.file =~ $canonical
RETURN m.file AS file,
       m.line AS line,
       t.qname AS enum_qname
ORDER BY file ASC, line ASC

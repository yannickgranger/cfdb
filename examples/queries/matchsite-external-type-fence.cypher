// smoke-skip: parameterized ($type_regex, $canonical) — a fence TEMPLATE; each repo instantiates it as a concrete arch-ban-*.cypher
// matchsite-external-type-fence.cypher — RFC-053 §3.5 / §3.6, slice 53-C.
//
// Split-resolution-point fence TEMPLATE, external-type form. A repo copies
// this shape into a concrete `examples/queries/arch-ban-<repo>-<type>.cypher`
// rule that pins `$type_regex` and `$canonical` to literals — see the live
// instantiation `arch-ban-rfc-053-syn-visibility-split-resolution.cypher`
// (cfdb's own `syn::Visibility` guard). It is NOT itself a gate: it is
// parameterized, so it is smoke-skipped and never enforced.
//
// # When to use this form (vs. the workspace-enum form)
//
// Use the EXTERNAL-type form when the fenced type has NO workspace `:Item`
// node — e.g. `syn::Visibility`, a type from a dependency crate. Such a
// match site carries a `:MatchSite` with a name-level `matched_path` but NO
// `MATCHES_ON` edge (there is no workspace enum to resolve to — RFC-053
// §3.2, the honest name-level-only representation). The name-level
// `matched_path` regex is therefore the only handle.
//
// For a WORKSPACE enum, use the sibling `matchsite-workspace-enum-fence.cypher`
// instead: it anchors on the resolved `MATCHES_ON` edge and is homonym-proof.
//
// # Homonym hazard — anchor `$type_regex` deliberately
//
// `matched_path` is the pattern-path prefix AS WRITTEN, not a resolved type
// (§3.1): `Visibility`, `syn::Visibility`, `a::b::Visibility` are three
// distinct values for potentially-different types. A same-named workspace
// enum matched unqualified yields the same bare `Visibility` prefix as an
// imported external type. The v0.1 query subset cannot express the
// principled complement "AND this site has no MATCHES_ON edge to a workspace
// enum" — it does not bind an outer-scope node inside `NOT EXISTS { ... }`
// (`vertical-split-brain-drop.cypher:67-73`). So separate the homonym at the
// pattern level: anchor `$type_regex` to the QUALIFIED path the external
// type is written as (`^syn::Visibility$`), not a bare `(^|::)Name$`, unless
// you have independently confirmed no workspace enum shares the name. When
// the name IS shared and unqualified uses exist, the workspace-enum form is
// the correct tool — it discriminates by resolved qname.
//
// # Guardrail (RFC-053 §3.6, verbatim from ratification)
//
// One fence file per fenced type; at most ONE canonical-site NOT-clause per
// file; never an accreting exception list. A rule file that grows
// NOT-clauses has become a de facto allowlist and is rejected on sight
// (project CLAUDE.md §3, no-ratchet rule). Multiplicity is a SURVEY metric,
// not a violation — matching your own type from many fns is normal; a fence
// exists only once a canonical owner is DESIGNATED. See
// `docs/split-resolution-fences.md`.
//
// # Parameters
//   $type_regex — Rust regex over `matched_path`, e.g. '^syn::Visibility$'
//   $canonical  — Rust regex over the workspace-relative `file`; the single
//                 designated owner to exempt, e.g.
//                 '.*crates/cfdb-extractor/src/item_visitor.*'
//
// # Usage
//   cfdb query --db <dir> --keyspace <ks> \
//     --params '{"type_regex":"^syn::Visibility$","canonical":".*crates/cfdb-extractor/src/item_visitor.*"}' \
//     "$(cat matchsite-external-type-fence.cypher)"
//
// Expected: empty when every match on the fenced type lives at the canonical
// site. Any row is a match on the type outside its designated owner.

//
// NOTE on `$canonical` / file regexes: node `file` props are ABSOLUTE
// paths in practice (the extractor's workspace-relative strip silently
// falls back to the absolute path whenever `--workspace` is not an
// exact prefix — e.g. CI's `--workspace .`). Always wrap file patterns
// as `.*<path fragment>.*`; an `^`-anchored relative path never matches
// and produces a silently-dead fence.
MATCH (m:MatchSite)
WHERE m.matched_path =~ $type_regex
  AND m.is_test = false
  AND NOT m.file =~ $canonical
RETURN m.file AS file,
       m.line AS line,
       m.matched_path AS matched_path
ORDER BY file ASC, line ASC

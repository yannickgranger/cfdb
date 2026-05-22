// arch-ban-rfc-043-resolver-domain.cypher
//   — RFC-044 §3.8 slice 044-H (issue #427); encodes the
//     Label::CALL_SITE discriminator contract closed-vocabulary clause
//     (RFC-029 §A1.2 / RFC-043 §4).
//
// # Invariant encoded (CallSite.resolver closed vocabulary)
//
// `cfdb_core::schema::Label::CALL_SITE` (`labels.rs:30-33`) pins the
// `resolver` discriminator to a CLOSED two-value vocabulary:
//
//     resolver ∈ {"syn", "hir"}
//
// Only two producers exist — `cfdb-extractor` (`"syn"`) and
// `cfdb-hir-extractor` (`"hir"`). Any other string is a wire-vocabulary
// drift: a third producer was wired in without an RFC + SchemaVersion
// review (no-ratchet rule, project CLAUDE.md §3 / §6.8), or a producer
// typo'd the literal. RFC-043 §4 keeps the dual-extractor population
// discriminable on this enum; before this rule the closed-set property was
// reviewer-only.
//
// # Inversion (positive invariant → its negation)
//
// Positive invariant: every :CallSite has `resolver` in the closed set.
// Negation: `NOT c.resolver IN ['syn','hir']` selects every node carrying
// an out-of-vocabulary resolver. A clean tree yields none.
//
// # Why the literal list is not a frozen split-brain
//
// The two values are owned by `Label::CALL_SITE`'s doc-contract in
// `cfdb-core` and are emitted as raw strings by the two extractors. There
// is no Rust enum with a `variants()` iterator to derive from at Cypher
// authoring time; the vocabulary is intentionally closed and adding a
// third value is an RFC-gated SchemaVersion change that would update THIS
// rule in the same PR (no-ratchet). The static-check unit test
// (`crates/cfdb-query/tests/arch_ban_rfc_rules_parse.rs`) keeps the rule's
// label/edge references in lockstep with the schema.
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
//     --rule examples/queries/arch-ban-rfc-043-resolver-domain.cypher
//
// Expected: empty on a clean tree. Any row is a CallSite whose `resolver`
// is outside the closed {"syn", "hir"} vocabulary.

MATCH (c:CallSite)
WHERE NOT c.resolver IN ['syn', 'hir']
RETURN c.caller_qname AS caller_qname,
       c.callee_path AS callee_path,
       c.resolver AS resolver,
       c.file AS file,
       c.line AS line
ORDER BY file ASC, line ASC

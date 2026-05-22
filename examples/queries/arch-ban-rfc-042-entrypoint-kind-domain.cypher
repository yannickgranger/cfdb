// arch-ban-rfc-042-entrypoint-kind-domain.cypher
//   — RFC-044 §3.8 slice 044-H (issue #427); encodes RFC-042 §4
//     (:EntryPoint.kind closed wire vocabulary).
//
// # Invariant encoded (RFC-042 §4 — EntryPoint.kind closed vocabulary)
//
// `:EntryPoint.kind` is a CLOSED enum vocabulary (`cfdb schema-describe`
// nodes.rs:299, and the raw literals emitted by
// `cfdb-hir-extractor::entry_point_emitter`):
//
//     kind ∈ {"mcp_tool", "cli_command", "http_route",
//             "cron_job", "websocket", "test", "bench"}
//
// Each value names a distinct entry-surface detector (clap derive →
// `cli_command`, `#[tool]` → `mcp_tool`, route chains → `http_route`,
// `#[test]`/cucumber → `test`, …). Any other string is wire-vocabulary
// drift: a new detector wired in without an RFC + SchemaVersion review
// (no-ratchet rule, project CLAUDE.md §3 / §6.8), or a typo'd literal in
// the emitter. RFC-042 §4 keeps this enum closed so downstream
// kind-filtering queries (e.g. `:Item.reachable_from_production_entry`,
// which excludes `kind ∈ {test, bench}`) stay sound; before this rule the
// closed-set property was reviewer-only.
//
// # Inversion (positive invariant → its negation)
//
// Positive invariant: every :EntryPoint.kind is in the closed set.
// Negation: `NOT e.kind IN [...]` selects every node carrying an
// out-of-vocabulary kind. A clean tree yields none.
//
// # Why the literal list is not a frozen split-brain
//
// The 7 values are the wire vocabulary owned by the extractor + schema
// descriptor in `cfdb-core` / `cfdb-hir-extractor`; they are emitted as raw
// strings (no Rust enum with a `variants()` iterator exists to derive from
// at Cypher authoring time). Adding an 8th detector kind is an RFC-gated
// SchemaVersion change that would update THIS rule in the same PR
// (no-ratchet). The static-check unit test
// (`crates/cfdb-query/tests/arch_ban_rfc_rules_parse.rs`) keeps the rule's
// schema-label/edge references in lockstep.
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
//     --rule examples/queries/arch-ban-rfc-042-entrypoint-kind-domain.cypher
//
// Expected: empty on a clean tree. Any row is a :EntryPoint whose `kind` is
// outside the closed wire vocabulary.

MATCH (e:EntryPoint)
WHERE NOT e.kind IN ['mcp_tool', 'cli_command', 'http_route', 'cron_job', 'websocket', 'test', 'bench']
RETURN e.name AS name,
       e.kind AS kind,
       e.handler_qname AS handler_qname,
       e.file AS file
ORDER BY file ASC, name ASC

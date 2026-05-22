// smoke-skip: template-driven ({{ declared_context_count }}, {{ declared_canonical_crate_count }}) — substituted by tools/dogfood-enrich (RFC-039 §3.1.2)
// self-enrich-concepts.cypher — RFC-039 §3.1.2 / §7.5 dogfood sentinel.
//
// Asserts that the concept-enrichment pipeline is faithful to the
// `.cfdb/concepts/*.toml` overrides loaded by `cfdb-concepts`:
//
//   (a) The number of `:Concept` nodes in the keyspace equals the number
//       of distinct context names declared across `.cfdb/concepts/*.toml`
//       (deduplicated by name, NOT by file — two TOML files declaring
//       the same `name = "..."` collapse to one concept).
//   (b) At least one `:LABELED_AS` edge exists. This is the "every Item
//       routed through `cfdb-concepts::lookup` carries a context label"
//       invariant — if zero edges exist, either the extractor stopped
//       emitting them or the concept-loader silently dropped its overrides.
//   (c) Conditional: IF the workspace declares at least one
//       `canonical_crate = "..."` value across its concept TOMLs, THEN
//       at least one `:CANONICAL_FOR` edge must exist. The conditional
//       is required because `ContextMeta.canonical_crate` is
//       `Option<String>` (`crates/cfdb-concepts/src/lib.rs:65`) — a
//       workspace whose every concept omits `canonical_crate` is a
//       legitimate empty case, and a hard `count(:CANONICAL_FOR) > 0`
//       assertion would false-positive on it.
//
// # Sentinel pattern
//
// The harness (`tools/dogfood-enrich`) walks `.cfdb/concepts/*.toml`
// before invoking the query. `tools/dogfood-enrich/src/scan_concepts.rs`
// produces:
//
//   distinct_context_names      → {{ declared_context_count }}
//   declared_canonical_crate_count → {{ declared_canonical_crate_count }}
//
// The query then evaluates all three sentinels in a single WHERE.
// One row is returned when ANY sentinel fails — `cfdb violations`
// translates "rows > 0" to exit 30.
//
// # Why anchor on `:Item`, not `:Concept`
//
// The cfdb-query Cypher subset evaluates `WITH count(c) AS k` over an
// empty MATCH binding by emitting ZERO output rows (the SQL/relational
// "no group => no row" rule, see
// `crates/cfdb-petgraph/src/eval/with_clause.rs::group_and_aggregate`).
// If we anchored the query on `MATCH (c:Concept)` and the extractor
// emitted zero `:Concept` nodes (a regression we MUST catch), the WITH
// would produce zero rows and the WHERE would never fire — the sentinel
// would silently pass on the very failure mode it must detect.
//
// We therefore anchor on `MATCH (i:Item)` (cfdb-self always has
// thousands of `:Item` nodes; the workspace having zero `:Item` is a
// catastrophic extractor failure already caught upstream) and use
// `OPTIONAL MATCH (c:Concept)` + `count(distinct c)` to count concepts
// without losing the anchor row.
//
// # Output columns (when invariant fails)
//
//   concept_count    — count of :Concept nodes in the keyspace
//   declared_count   — count of distinct context names declared across
//                      .cfdb/concepts/*.toml (substituted from the
//                      harness-side TOML scan)
//
// # Usage
//
//   ./target/release/dogfood-enrich --pass enrich-concepts \
//       --db .cfdb/db --keyspace cfdb-self \
//       --cfdb-bin ./target/release/cfdb \
//       --workspace .
//
// Expected on cfdb-self: 0 rows. Any row is a concept-enrichment
// regression and surfaces as exit 30.

MATCH (i:Item)
OPTIONAL MATCH (c:Concept)
WITH count(distinct c) AS concept_count
WHERE concept_count <> {{ declared_context_count }}
   OR NOT EXISTS { MATCH (:Item)-[:LABELED_AS]->(:Concept) }
   OR ({{ declared_canonical_crate_count }} > 0 AND NOT EXISTS { MATCH (:Item)-[:CANONICAL_FOR]->(:Concept) })
RETURN concept_count, {{ declared_context_count }} AS declared_count

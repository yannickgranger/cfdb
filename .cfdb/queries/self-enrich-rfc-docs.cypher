// smoke-skip: template-driven ({{ ground_truth_count }}) — substituted by tools/dogfood-enrich (RFC-039 §3.1)
// self-enrich-rfc-docs.cypher — RFC-039 §3.1 / §7.3 dogfood sentinel.
//
// Asserts two invariants against the cfdb-self keyspace, per RFC-039
// §3.1 row `enrich-rfc-docs`:
//
//   (a) count(:RfcDoc) >= count(docs/RFC-*.md)  — every RFC markdown
//       file on disk has been ingested as an :RfcDoc node.
//   (b) count(:Item)-[:REFERENCED_BY]->(:RfcDoc) > 0  — at least one
//       extracted symbol has a back-reference into an RFC body.
//
// The harness (`tools/dogfood-enrich`) walks the workspace, counts
// `docs/RFC-*.md` files via the helper in
// `tools/dogfood-enrich/src/grep_rfc_docs.rs`, and substitutes the
// count into the `{{ ground_truth_count }}` placeholder below before
// submission. Issue #344 is the implementing slice.
//
// # Sentinel pattern
//
// Both invariants evaluate in a single WHERE clause; the query returns
// one row when EITHER sentinel fires:
//
//   - Sentinel (a) fires when the extracted :RfcDoc count is strictly
//     less than the FS ground truth. This catches the case where an
//     `RFC-NNN-*.md` file was added on disk but the extractor's
//     RFC-doc producer did not pick it up (recall regression).
//   - Sentinel (b) fires when zero outgoing REFERENCED_BY edges exist
//     into any :RfcDoc. This catches the case where the RFC ingestion
//     landed but the cross-reference producer is silently dead.
//
// When both invariants hold, the query returns 0 rows and the harness
// exits 0. Either invariant firing returns 1 row → harness exits 30.
//
// # Why anchor on `:Item`, not `:RfcDoc`
//
// The cfdb-query Cypher subset evaluates `WITH count(d) AS k` over an
// empty MATCH binding by emitting ZERO output rows (the SQL/relational
// "no group => no row" rule, see
// `crates/cfdb-petgraph/src/eval/with_clause.rs::group_and_aggregate`).
// If we anchored on `MATCH (d:RfcDoc)` and the extractor emitted zero
// `:RfcDoc` nodes (the catastrophic regression we MUST catch), the WITH
// would produce zero rows and the WHERE would never fire — the sentinel
// would silently pass on the failure mode it must detect.
//
// We therefore anchor on `MATCH (i:Item)` (cfdb-self always has
// thousands of `:Item` nodes; the workspace having zero `:Item` is a
// catastrophic extractor failure caught upstream) and use
// `OPTIONAL MATCH (d:RfcDoc)` + `count(distinct d)` to count RFC docs
// without losing the anchor row. UNION is NOT in the cfdb-query v0.1
// subset (verified at `crates/cfdb-query/src/parser/`); the OR-of-
// EXISTS pattern below is the equivalent shape, mirroring
// `self-enrich-concepts.cypher`.
//
// # Direction-of-comparison rationale
//
//   - Extracted < ground_truth → extractor missed an RFC file. RED.
//   - Extracted > ground_truth → harness's FS scan missed a file
//     (false negative in `count_rfc_md_files`). This direction is NOT
//     flagged — the dogfood gate is about extractor recall, not
//     source-text scan precision.
//
// # Output columns (when invariant fails)
//
//   rfc_doc_count   — count of :RfcDoc nodes in the keyspace
//   source_count    — substituted ground truth (FS scan)
//
// # Usage
//
//   ./target/release/dogfood-enrich --pass enrich-rfc-docs \
//       --db .cfdb/db --keyspace cfdb-self \
//       --cfdb-bin ./target/release/cfdb \
//       --workspace .
//
// Expected on cfdb-self: 0 rows. Any row is a recall regression.

MATCH (i:Item)
OPTIONAL MATCH (d:RfcDoc)
WITH count(distinct d) AS rfc_doc_count
WHERE rfc_doc_count < {{ ground_truth_count }}
   OR NOT EXISTS { MATCH ()-[:REFERENCED_BY]->(:RfcDoc) }
RETURN rfc_doc_count, {{ ground_truth_count }} AS source_count

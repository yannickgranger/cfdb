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
// Cypher does not support multi-row conditional returns natively, so
// the two invariants are encoded as a UNION of two single-row checks:
//
//   - Sentinel (a) fires when the extracted :RfcDoc count is strictly
//     less than the FS ground truth. This catches the case where an
//     `RFC-NNN-*.md` file was added on disk but the extractor's
//     RFC-doc producer did not pick it up (recall regression).
//   - Sentinel (b) fires when there is at least one :RfcDoc node but
//     zero outgoing REFERENCED_BY edges from any :Item. This catches
//     the case where the RFC ingestion landed but the cross-reference
//     producer is silently dead (edge regression).
//
// When both invariants hold, the UNION returns 0 rows and the harness
// exits 0. Either invariant firing returns 1 row and the harness
// exits 30.
//
// # Direction-of-comparison rationale
//
//   - Extracted < ground_truth → extractor missed an RFC file. RED.
//   - Extracted > ground_truth → harness's FS scan missed a file
//     (false negative in `count_rfc_md_files`; e.g. a non-glob path
//     traversal). This direction is NOT flagged here — the dogfood
//     gate is about extractor recall, not source-text scan precision.
//
// # Output columns (when invariant fails)
//
//   sentinel        — 'rfc_doc_count' or 'referenced_by_edges'
//   extracted_count — count of :RfcDoc nodes / REFERENCED_BY edges
//   source_count    — substituted ground truth (sentinel a only;
//                     placeholder 0 for sentinel b which has no FS
//                     ground truth, only a strict-positive check)
//
// # Usage
//
//   ./target/release/dogfood-enrich --pass enrich-rfc-docs \
//       --db .cfdb/db --keyspace cfdb-self \
//       --cfdb-bin ./target/release/cfdb \
//       --workspace .
//
// Expected on cfdb-self: 0 rows. Any row is a recall regression.

MATCH (d:RfcDoc)
WITH count(d) AS rfc_doc_count
WHERE rfc_doc_count < {{ ground_truth_count }}
RETURN 'rfc_doc_count' AS sentinel,
       rfc_doc_count AS extracted_count,
       {{ ground_truth_count }} AS source_count
UNION
MATCH ()-[e:REFERENCED_BY]->(:RfcDoc)
WITH count(e) AS edge_count
WHERE edge_count = 0
RETURN 'referenced_by_edges' AS sentinel,
       edge_count AS extracted_count,
       0 AS source_count

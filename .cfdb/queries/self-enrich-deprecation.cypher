// self-enrich-deprecation.cypher — RFC-039 §3.1 / §7.2 dogfood sentinel.
//
// Asserts that every `#[deprecated]` attribute in the workspace source
// has a matching `:Item.is_deprecated = true` in the extracted graph.
// The extractor populates `is_deprecated` extractor-time per
// `cfdb-extractor::extract_deprecated_attr` (#106 / RFC addendum
// §A2.2 row 3); the corresponding `EnrichBackend::enrich_deprecation`
// is a non-stub no-op naming the extractor as the source.
//
// # Sentinel pattern
//
// The harness (`tools/dogfood-enrich`) walks the workspace, counts
// `#[deprecated]` occurrences via the regex helper in
// `tools/dogfood-enrich/src/grep_deprecated.rs`, and substitutes the
// count into the `{{ ground_truth_count }}` placeholder below. The
// query then compares the extracted-graph count against the source-
// side ground truth. When the extractor undercounts (a regression
// that drops one `#[deprecated]` annotation), the WITH/WHERE clause
// returns one row and the harness exits 30.
//
// Direction-of-comparison rationale:
//   - Extracted < ground_truth → extractor missed one. RED.
//   - Extracted > ground_truth → grep missed one (false negative
//     in the regex; e.g. a `#[deprecated]` inside a comment that
//     the regex picked up but `extract_deprecated_attr` filtered).
//     This direction is NOT flagged here — the dogfood gate is
//     about extractor recall, not source-text grep precision.
//
// # Output columns (when invariant fails)
//
//   extracted_count — count of :Item nodes with is_deprecated = true
//   source_count    — count of #[deprecated] occurrences in workspace
//
// # Usage
//
//   ./target/release/dogfood-enrich --pass enrich-deprecation \
//       --db .cfdb/db --keyspace cfdb-self \
//       --cfdb-bin ./target/release/cfdb \
//       --workspace .
//
// Expected on cfdb-self: 0 rows. Any row is a recall regression.

MATCH (i:Item)
WHERE i.is_deprecated = true
WITH count(i) AS extracted_count
WHERE extracted_count < {{ ground_truth_count }}
RETURN extracted_count, {{ ground_truth_count }} AS source_count

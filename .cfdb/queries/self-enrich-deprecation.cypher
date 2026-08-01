// smoke-skip: template-driven ({{ ground_truth_count }}) — substituted by tools/dogfood-enrich (RFC-039 §3.1)
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
// The harness (`tools/dogfood-enrich`) reads the keyspace's `:File`
// node set (the files the extractor actually walked), counts
// attribute-position `#[deprecated]` occurrences in exactly those
// files — comments and string/char literals lexically stripped first
// (`tools/dogfood-enrich/src/grep_deprecated.rs`) — and substitutes
// the count into the `{{ ground_truth_count }}` placeholder below.
// The query then compares the extracted-graph count against the
// source-side ground truth. When the extractor undercounts (a
// regression that drops one `#[deprecated]` annotation in a walked
// file), the WITH/WHERE clause returns one row and the harness
// exits 30.
//
// Ground-truth scoping (PR #563 correction): the original raw-text
// workspace grep counted doc comments, test string literals, and
// fixture crates the extractor never walks — 73 raw matches vs 1
// genuine attribute at the time of the fix. The mismatch was masked
// by #564 (count() over an empty MATCH yields no rows, so the WHERE
// never evaluated while cfdb-self had zero deprecated items). Both
// the stripping and the `:File`-set scoping exist to make the two
// sides of this comparison share one universe.
//
// Direction-of-comparison rationale:
//   - Extracted < ground_truth → extractor missed one. RED.
//   - Extracted > ground_truth → the lexical stripper over-stripped
//     (a real attribute misread as literal/comment content). This
//     direction is NOT flagged here — the dogfood gate is about
//     extractor recall, not stripper precision.
//   - Extracted = 0 with ground_truth ≥ 1 → the template CANNOT fire
//     (#564: no count row on empty MATCH); the harness's
//     zero-extracted guard in `main.rs` owns that case and exits 30
//     directly.
//   - Known granularity landmine (#565): `#[deprecated]` on a struct
//     field or enum variant counts source-side but is
//     unrepresentable graph-side (`is_deprecated` lives on `:Item`
//     only) — the first such attribute turns this gate RED with no
//     extractor regression. Fails closed; schema fix is RFC-gated.
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

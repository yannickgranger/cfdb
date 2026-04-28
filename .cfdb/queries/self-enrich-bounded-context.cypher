// smoke-skip: template-driven ({{ nulls_threshold }}, {{ total_items }}) — substituted by tools/dogfood-enrich (RFC-039 §3.1.1; #355 Path B)
// self-enrich-bounded-context.cypher — RFC-039 §3.1.1 / §7.4 / §I3.1 dogfood sentinel.
//
// Asserts that ≥ MIN_BC_COVERAGE_PCT% of `:Item` nodes carry a non-empty
// `bounded_context` after the combined extract+enrich pipeline. The
// sentinel measures combined-pipeline coverage (RFC-039 §3.1.1) — the
// `enrich_bounded_context` pass is a delta-patch, not a producer
// (`crates/cfdb-petgraph/src/enrich/bounded_context.rs:12-20`), so the
// invariant is on the keyspace state AFTER the pass has materialized,
// not on the pass's delta in isolation.
//
// # Sentinel pattern
//
// The cfdb-query v0.1 subset has no arithmetic operators (Add/Sub/Mul/Div)
// — the natural ratio `(total - nulls) / total < threshold / 100`
// cannot be expressed in Cypher. Path B from issue #355 resolves this
// at harness time:
//
//   1. `tools/dogfood-enrich/src/count_items.rs` subprocess-invokes
//      `cfdb query` to read `count(:Item) AS total_items`.
//   2. `compute_extra_substitutions` in `main.rs` computes
//      `nulls_threshold = total_items * (100 - threshold_pct) / 100`
//      where `threshold_pct = BC_COVERAGE_THRESHOLD` (95).
//   3. Both values are substituted into this template before submission.
//
// The Cypher then asserts a flat absolute-count comparison — the ratio
// is encoded in the substituted threshold, not the query.
//
// # Anchoring + empty-result behavior
//
// `MATCH (i:Item) WHERE i.bounded_context = ""` may bind zero rows when
// every `:Item` carries a non-empty `bounded_context` (the post-enrich
// happy path on cfdb-self at HEAD). Under the cfdb-query subset's
// SQL no-group-no-row rule (see
// `crates/cfdb-petgraph/src/eval/with_clause.rs::group_and_aggregate`),
// `WITH count(i) AS empty_count` then produces ZERO output rows, the
// WHERE never fires, and the harness sees zero violations — the desired
// "invariant holds" state. When ≥1 `:Item` carries an empty string,
// the WITH produces exactly one row carrying the count, and the
// comparison `empty_count > {{ nulls_threshold }}` decides pass/fail.
//
// # Why "empty string" instead of `IS NULL`
//
// `crates/cfdb-extractor/src/item_visitor/emit/mod.rs:257-260` always
// emits `bounded_context` as a `PropValue::Str` — never absent, never
// `None`. The realistic regression mode is the empty-string sentinel
// (the `cfdb-concepts` resolver returning `""` for an unmapped crate),
// not an absent property. `crates/cfdb-petgraph/src/eval/predicate.rs`'s
// `compare_propvalues` returns `false` when either side of `=` is
// `None`, so an `IS NULL` test would be a no-op against the actual
// extractor output.
//
// # Output columns (when invariant fails)
//
//   empty_count     — count of :Item nodes with bounded_context = ""
//   nulls_threshold — substituted absolute threshold derived from
//                     `total_items * (100 - BC_COVERAGE_THRESHOLD) / 100`
//   total_items     — substituted total :Item count (context for the
//                     reviewer; the actual gating uses nulls_threshold)
//
// # Usage
//
//   ./target/release/dogfood-enrich --pass enrich-bounded-context \
//       --db .cfdb/db --keyspace cfdb-self \
//       --cfdb-bin ./target/release/cfdb \
//       --workspace .
//
// Expected on cfdb-self: 0 rows. Any row is a coverage regression.

MATCH (i:Item)
WHERE i.bounded_context = ""
WITH count(i) AS empty_count
WHERE empty_count > {{ nulls_threshold }}
RETURN empty_count,
       {{ nulls_threshold }} AS nulls_threshold,
       {{ total_items }} AS total_items

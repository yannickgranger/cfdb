// smoke-skip: template-driven ({{ total_items }}, {{ nulls_threshold }}) — substituted by tools/dogfood-enrich (RFC-039 §3.1; #355 Path B)
// self-enrich-metrics.cypher — RFC-039 §3.1 / §7.7 / issue #348 dogfood sentinel.
//
// Asserts that ≥ MIN_METRICS_COVERAGE_PCT% of `:Item{kind:"fn"}` nodes
// carry BOTH `cyclomatic` AND `unwrap_count` after the
// `enrich_metrics` pass. The two attrs are written together by
// `crates/cfdb-petgraph/src/enrich/metrics/mod.rs::apply_item_attrs`
// (lines 194-204 — both inserted under the same
// `if let Some(sig) = signals.get(...)` guard), so a single
// "missing-attr" predicate covers both. Threshold const is
// `METRICS_COVERAGE_THRESHOLD = Some(95)` in
// `tools/dogfood-enrich/src/thresholds.rs:55`.
//
// # Sentinel pattern
//
// The cfdb-query v0.1 subset has no arithmetic operators (Add/Sub/
// Mul/Div) — the natural ratio `nulls / total < threshold / 100`
// cannot be expressed in Cypher. Path B from issue #355 resolves
// this at harness time:
//
//   1. `tools/dogfood-enrich/src/count_items.rs` (kind=fn variant,
//      to be added in the bundle PR per #348 deferred scope)
//      subprocess-invokes `cfdb query` to read
//      `count(:Item{kind:"fn"}) AS total_items`.
//   2. `compute_extra_substitutions` in `main.rs` computes
//      `nulls_threshold = total * (100 - threshold_pct) / 100`
//      where `threshold_pct = METRICS_COVERAGE_THRESHOLD` (95).
//   3. Both values are substituted into this template before
//      submission.
//
// The Cypher then asserts a flat absolute-count comparison — the
// ratio is encoded in the substituted threshold, not the query.
//
// # Why "NOT (cyclomatic >= 1)" instead of "IS NULL"
//
// `crates/cfdb-query/src/parser/predicate.rs` does NOT support
// `IS NULL` / `IS NOT NULL` in the v0.1 subset (verified — the
// parser exposes only `NOT EXISTS { MATCH ... }` for absence
// detection, and that subquery cannot reference outer bindings).
// Direct equality against a sentinel value (`= 0`) does NOT detect
// missing properties: `crates/cfdb-petgraph/src/eval/predicate.rs`'s
// `compare_propvalues` (lines 514-517) returns `false` whenever
// either side is `None`, so `i.cyclomatic = 0` against a node
// missing the prop yields `false`, not a match.
//
// The shape `NOT (i.cyclomatic >= 1)` exploits this same
// semantics in reverse:
//
//   - When `cyclomatic` IS set, the value is always ≥ 1
//     (`crates/cfdb-petgraph/src/enrich/metrics/ast_signals.rs:131`
//     computes `cyclomatic = branches + 1`; the trivial-fn unit
//     test at line 193-194 asserts `cyclomatic == 1`). So
//     `i.cyclomatic >= 1` is `true` for every enriched node, and
//     `NOT (i.cyclomatic >= 1)` is `false` — node does NOT match.
//   - When `cyclomatic` is absent, `i.cyclomatic >= 1` evaluates
//     `compare_propvalues(Ge, None, Some(1))` → `false`. The
//     outer `NOT` flips this to `true` → node matches.
//
// Same logic applies to `i.unwrap_count >= 0` (the value is
// always ≥ 0 when set, since `ast_signals::AstSignals.unwrap_count`
// is a `usize`).
//
// # Anchoring + empty-result behavior
//
// `MATCH (i:Item) WHERE i.kind = "fn" AND (...missing predicate...)`
// may bind zero rows when every fn node carries both metrics
// (the post-enrich happy path on cfdb-self at HEAD). Under the
// cfdb-query subset's SQL no-group-no-row rule (see
// `crates/cfdb-petgraph/src/eval/with_clause.rs::group_and_aggregate`),
// `WITH count(i) AS missing_count` then produces ZERO output
// rows, the outer WHERE never fires, and the harness sees zero
// violations — the desired "invariant holds" state. When ≥1 fn
// node is missing either metric, the WITH produces exactly one
// row carrying the count, and `missing_count > {{ nulls_threshold }}`
// decides pass/fail.
//
// # Output columns (when invariant fails)
//
//   missing_count   — count of :Item{kind:"fn"} nodes missing
//                     either `cyclomatic` or `unwrap_count`
//   nulls_threshold — substituted absolute threshold derived from
//                     `total_items * (100 - METRICS_COVERAGE_THRESHOLD) / 100`
//   total_items     — substituted total :Item{kind:"fn"} count
//                     (context for the reviewer; the actual gating
//                     uses nulls_threshold)
//
// # Usage
//
//   ./target/release/dogfood-enrich --pass enrich-metrics \
//       --db .cfdb/db --keyspace cfdb-self \
//       --cfdb-bin ./target/release/cfdb \
//       --workspace .
//
// Expected on cfdb-self post-enrich: 0 rows. Any row is a coverage
// regression — `enrich_metrics` failed to populate ast_signals on
// > 5% of fn items.

MATCH (i:Item)
WHERE i.kind = "fn"
  AND (NOT (i.cyclomatic >= 1) OR NOT (i.unwrap_count >= 0))
WITH count(i) AS missing_count
WHERE missing_count > {{ nulls_threshold }}
RETURN missing_count,
       {{ nulls_threshold }} AS nulls_threshold,
       {{ total_items }} AS total_items

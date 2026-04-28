// smoke-skip: template-driven ({{ total_items }}, {{ nulls_threshold }}) — substituted by tools/dogfood-enrich (RFC-039 §3.1; #355 Path B)
// self-enrich-git-history.cypher — RFC-039 §3.1 / §7.8 / issue #349 dogfood sentinel.
//
// Asserts that ≥ MIN_GIT_COVERAGE_PCT% of `:Item` nodes carry a non-null
// `git_last_commit_unix_ts` after the `enrich_git_history` pass. The
// sentinel measures combined-pipeline coverage (RFC-039 §3.1) — the pass
// always writes the attribute on every `:Item`, either as
// `PropValue::Int(epoch_seconds)` for files seen by HEAD's history or as
// `PropValue::Null` for items whose `file` is outside the git tree
// (vendored deps, generated code, items emitted with no `file` prop).
// See `crates/cfdb-petgraph/src/enrich/git_history.rs:213-235`
// (`write_attrs_one`) — the always-3-write contract is load-bearing for
// this template's `= null` test.
//
// # Attribute name correction
//
// The R1 RFC-039 draft cited `commit_age_days` as the per-item attribute.
// That attribute does NOT exist. The actual emitted attribute is
// `git_last_commit_unix_ts` (epoch seconds, `PropValue::Int(i64)`),
// declared at `crates/cfdb-petgraph/src/enrich/git_history.rs:51`
// (`pub(crate) const ATTR_TS: &str = "git_last_commit_unix_ts"`).
// `tools/dogfood-enrich/src/thresholds.rs:61-69` records the correction
// inline. rust-systems caught the typo during architect review.
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
//      where `threshold_pct = GIT_COVERAGE_THRESHOLD` (95).
//   3. Both values are substituted into this template before submission.
//
// The Cypher then asserts a flat absolute-count comparison — the ratio
// is encoded in the substituted threshold, not the query.
//
// # Anchoring + null-equivalent test
//
// `MATCH (i:Item)` with no kind filter — git history applies to every
// `:Item` per the issue #349 body. The null-equivalent test is
// `WHERE i.git_last_commit_unix_ts = null`:
//
//   * `crates/cfdb-query/src/parser/lexical.rs:152-158` parses the
//     `null` keyword as `PropValue::Null`.
//   * `crates/cfdb-petgraph/src/eval/predicate.rs:509-533`
//     (`compare_propvalues`) returns `true` for `Eq` only when both
//     operands are `PropValue::Null` (line 531). Any `Int` vs `Null`
//     comparison falls through to the `_ => return false` arm at
//     line 532, so non-null items never match.
//   * Items where the property is genuinely absent (`Option::None` from
//     `props.get`) bail at line 516 — they are NOT counted as null.
//     The `enrich_git_history` pass's always-3-write contract
//     (git_history.rs:213-235) guarantees the attribute is present on
//     every `:Item`, so absence is a regression separate from coverage.
//
// `IS NULL` syntax is intentionally NOT used because the cfdb-query v0.1
// subset does not parse it (search `crates/cfdb-query/src/parser/` for
// `IS NULL` — zero hits). The literal `= null` form is the only
// expressible null test in the subset and matches the BC sibling's
// equality-against-sentinel approach (`self-enrich-bounded-context.cypher`
// uses `= ""`; here we use `= null` because the extractor's "missing"
// shape is `PropValue::Null`, not an empty string).
//
// # Empty-result behavior
//
// When every `:Item` carries a non-null `git_last_commit_unix_ts` (the
// post-enrich happy path on cfdb-self at HEAD inside a git workspace),
// `MATCH (i:Item) WHERE i.git_last_commit_unix_ts = null` binds zero
// rows. Under the cfdb-query subset's SQL no-group-no-row rule
// (`crates/cfdb-petgraph/src/eval/with_clause.rs::group_and_aggregate`),
// `WITH count(i) AS null_count` then produces ZERO output rows, the
// downstream WHERE never fires, and the harness sees zero violations —
// the desired "invariant holds" state. When ≥1 `:Item` carries a Null
// timestamp, the WITH produces exactly one row carrying the count, and
// the comparison `null_count > {{ nulls_threshold }}` decides pass/fail.
//
// # Output columns (when invariant fails)
//
//   null_count      — count of :Item nodes with git_last_commit_unix_ts = null
//   nulls_threshold — substituted absolute threshold derived from
//                     `total_items * (100 - GIT_COVERAGE_THRESHOLD) / 100`
//   total_items     — substituted total :Item count (context for the
//                     reviewer; the actual gating uses nulls_threshold)
//
// # Usage
//
//   ./target/release/dogfood-enrich --pass enrich-git-history \
//       --db .cfdb/db --keyspace cfdb-self \
//       --cfdb-bin ./target/release/cfdb \
//       --workspace .
//
// Expected on cfdb-self: 0 rows. Any row is a coverage regression.

MATCH (i:Item)
WHERE i.git_last_commit_unix_ts = null
WITH count(i) AS null_count
WHERE null_count > {{ nulls_threshold }}
RETURN null_count,
       {{ nulls_threshold }} AS nulls_threshold,
       {{ total_items }} AS total_items

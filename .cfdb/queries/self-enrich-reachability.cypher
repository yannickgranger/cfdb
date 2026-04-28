// smoke-skip: template-driven ({{ total_items }}, {{ nulls_threshold }}) — substituted by tools/dogfood-enrich (RFC-039 §3.1; #355 Path B)
// self-enrich-reachability.cypher — RFC-039 §3.1 / §7.6 / issue #347 dogfood sentinel.
//
// Asserts that ≥ MIN_REACHABILITY_PCT% of `:Item{kind:"fn"}` nodes are
// reachable from at least one `:EntryPoint` over the call graph after
// the combined `cfdb extract --features hir` + `cfdb enrich-reachability`
// pipeline. The pass writes `:Item.reachable_from_entry` (bool) per
// `crates/cfdb-petgraph/src/enrich/reachability.rs:72,202` — that
// attribute is the gating signal here.
//
// # Why we read `reachable_from_entry`, not traverse `CALLS*`
//
// The natural shape of this invariant is a variable-length path from
// every `:EntryPoint` over `[:CALLS*]` to every `:Item{kind:"fn"}`,
// asserting that ≥ N% of fns are touched. The cfdb-query v0.1 subset
// has no variable-length path patterns (the `*` quantifier is unparsed
// — see RFC-cfdb / cfdb-petgraph eval surface), so the BFS cannot be
// expressed in Cypher. The `enrich_reachability` pass already runs the
// BFS in Rust and materializes its result on every `:Item` node as a
// flat boolean (`reachable_from_entry`) — the sentinel reads that
// attribute and counts unreachable fns. Traversal happens once,
// extractor-time; the dogfood query is O(N) over `:Item` nodes.
//
// # Sentinel pattern (Path B from issue #355)
//
// The cfdb-query v0.1 subset also lacks arithmetic operators
// (Add/Sub/Mul/Div), so the natural ratio
// `(unreachable_fn_count / total_fn_count) > (100 - threshold) / 100`
// cannot be expressed in Cypher. Path B from #355 resolves this at
// harness time:
//
//   1. `tools/dogfood-enrich/src/count_items.rs` (or a kind-filtered
//      variant added at integration of #347 — see "Assumed
//      substitutions" below) subprocess-invokes `cfdb query` to read
//      `count(:Item{kind:"fn"}) AS total_items`.
//   2. `compute_extra_substitutions` in `main.rs` computes
//      `nulls_threshold = total_items * (100 - threshold_pct) / 100`
//      where `threshold_pct = REACHABILITY_THRESHOLD` (= 80, see
//      `tools/dogfood-enrich/src/thresholds.rs:50`).
//   3. Both values are substituted into this template before submission.
//
// The Cypher then asserts a flat absolute-count comparison —
// `unreachable_fn_count > nulls_threshold` — the ratio is encoded in
// the substituted threshold, not the query.
//
// # Assumed substitutions (integration contract)
//
// This template references `{{ total_items }}` and `{{ nulls_threshold }}`,
// matching the placeholder names already used by
// `self-enrich-bounded-context.cypher`. The bounded-context arm of
// `compute_extra_substitutions` counts ALL `:Item` nodes (kind-
// agnostic). Reachability scopes its denominator to fn items only —
// integration of #347 adds an `enrich-reachability` arm that either
// (a) calls a new `count_items_with_kind(cfdb_bin, db, keyspace, "fn")`
// helper, or (b) extends `count_items_in_keyspace` to take an optional
// kind filter. Either way, the substituted `{{ total_items }}` here
// MUST be the count of `:Item{kind:"fn"}`, not all items, otherwise
// the threshold is computed against the wrong denominator and the
// sentinel is unsound.
//
// # Anchoring + empty-result behavior
//
// `MATCH (i:Item) WHERE i.kind = "fn" AND i.reachable_from_entry = false`
// may bind zero rows on a happy-path keyspace where every fn is
// reachable. Under the cfdb-query subset's SQL no-group-no-row rule
// (`crates/cfdb-petgraph/src/eval/with_clause.rs::group_and_aggregate`),
// `WITH count(i) AS unreachable_fn_count` then produces ZERO output
// rows, the WHERE never fires, and the harness sees zero violations —
// the desired "invariant holds" state. When ≥1 fn is unreachable, the
// WITH produces exactly one row carrying the count, and the comparison
// `unreachable_fn_count > {{ nulls_threshold }}` decides pass/fail.
//
// We anchor on the unfiltered `MATCH (i:Item)` shape (with the kind
// + reachability filter pushed into the WHERE) for parity with
// `self-enrich-bounded-context.cypher` — the bounded-context header
// documents the same SQL no-group-no-row constraint and the rationale
// applies identically here.
//
// # Why bool comparison, not `IS NULL` / `NOT i.reachable_from_entry`
//
// `enrich_reachability` writes `reachable_from_entry` for every
// `:Item` node (`reachability.rs:200-205` — items with `count == 0`
// are explicitly marked `false`, never left absent). The realistic
// regression mode is `false` on items that should be `true`, not an
// absent property. An equality comparison (`= false`) matches the
// shape used by `self-enrich-deprecation.cypher` (`= true`) and
// stays inside the verified subset surface (`compare_propvalues` in
// `crates/cfdb-petgraph/src/eval/predicate.rs`).
//
// # Degraded-path interaction
//
// If the keyspace was extracted without `--features hir`, the pass
// runs in degraded mode and writes nothing (`reachability.rs:80-91`).
// The I5.1 feature-presence guard in
// `tools/dogfood-enrich/src/feature_guard.rs` rejects this case before
// the template is materialized, so the sentinel never sees a half-
// enriched keyspace.
//
// # Output columns (when invariant fails)
//
//   unreachable_fn_count — count of :Item{kind:"fn"} with
//                          reachable_from_entry = false
//   nulls_threshold      — substituted absolute threshold derived from
//                          `total_items * (100 - REACHABILITY_THRESHOLD) / 100`
//   total_items          — substituted count of :Item{kind:"fn"}
//                          (context for the reviewer; the actual gating
//                          uses nulls_threshold)
//
// # Usage
//
//   ./target/release/dogfood-enrich --pass enrich-reachability \
//       --db .cfdb/db --keyspace cfdb-self \
//       --cfdb-bin ./target/release/cfdb
//
// Expected on cfdb-self after `cfdb extract --features hir`:
// 0 rows. Any row is a reachability regression — either the HIR
// extractor lost CALLS edges or new entry-pointless code landed.

MATCH (i:Item)
WHERE i.kind = "fn"
  AND i.reachable_from_entry = false
WITH count(i) AS unreachable_fn_count
WHERE unreachable_fn_count > {{ nulls_threshold }}
RETURN unreachable_fn_count,
       {{ nulls_threshold }} AS nulls_threshold,
       {{ total_items }} AS total_items

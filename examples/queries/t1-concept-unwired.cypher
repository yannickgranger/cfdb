// t1-concept-unwired.cypher — Trigger T1 `:Context` inventory read.
//
// Dumps every `:Context` node with its three TOML-sourced props. The
// consumer (`cfdb check --trigger T1`) correlates this row set against
// three other graph reads (`:Crate.name`, `:Item.bounded_context`,
// `:RfcDoc.{path,title}`) in Rust to compute the three T1 sub-verdicts:
// CONCEPT_UNWIRED, MISSING_CANONICAL_CRATE, STALE_RFC_REFERENCE.
//
// # Why the verdict computation lives in Rust, not in this cypher file
//
// The three T1 sub-verdicts are anti-join queries — "contexts whose
// canonical_crate does NOT match any crate", etc. Anti-joins in the
// cfdb-query v0.1 subset have two limitations that make a pure-cypher
// expression fragile:
//
//   1. `NOT EXISTS { MATCH ... WHERE outer-ref }` — the inner subquery
//      runs in a fresh evaluator scope and cannot reference the outer
//      bindings, so `WHERE k.name = c.canonical_crate` inside the
//      subquery evaluates `c.canonical_crate` to None and the NOT
//      EXISTS is always true.
//
//   2. `OPTIONAL MATCH (k:Crate) WHERE k.name = c.canonical_crate` +
//      `WITH count(k) AS hits WHERE hits = 0` — the top-level WHERE
//      filters the cross-product BEFORE the WITH aggregation, so
//      outer-row rows that don't find a matching inner are dropped
//      rather than null-filled. Contexts whose canonical crate is
//      missing vanish instead of surfacing.
//
//   3. `WITH c, collect(k.name) AS all_crates WHERE NOT c.canonical_crate
//      IN all_crates` — aggregation-produced list bindings are not
//      addressable by the `IN` predicate (the evaluator's
//      `eval_expr_list` handles literal lists and `$param` bindings,
//      not `RowValue::List` bindings from `collect()`).
//
// Rather than fight those limitations with fragile workarounds, the
// check verb runs this inventory cypher + three small primitive reads
// and applies the anti-join logic in Rust. Each of the four cypher
// reads is self-standing and deterministic; the correlation is a
// closed, typed computation in `cfdb_cli::check`.
//
// The same pattern is used by `classifier-*.cypher` in this directory
// when anti-join semantics were needed — see the README of those
// rules.
//
// # Output row shape
//
// `{context_name, canonical_crate, owning_rfc}` — three columns per
// declared context. `canonical_crate` and `owning_rfc` are null when
// the source TOML did not set them (rare but legitimate).
//
// # Determinism
//
// `ORDER BY context_name ASC` — stable row order required by the
// dogfood determinism CI recipe.

MATCH (c:Context)
RETURN c.name AS context_name,
       c.canonical_crate AS canonical_crate,
       c.owning_rfc AS owning_rfc
ORDER BY context_name ASC

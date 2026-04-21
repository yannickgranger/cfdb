// t3-concept-multi-crate.cypher — Trigger T3 raw detection.
//
// Emits one row per distinct `:Item.name` (restricted to `struct`, `enum`,
// `trait`) that appears in items belonging to ≥2 distinct `:Crate`s. Each
// row carries:
//   - `name`, `kind` — the offending concept identity
//   - `n`            — total number of matching :Item nodes
//   - `n_crates`     — count of distinct crates holding the name
//   - `n_contexts`   — count of distinct bounded_contexts across those crates
//   - `crates[]`     — sorted, distinct list of crate names
//   - `bounded_contexts[]` — sorted, distinct list of bounded_context values
//   - `qnames[]`     — fully-qualified names per definition
//   - `files[]`      — source file per definition
//
// Consumed by `cfdb check --trigger T3`, which derives the per-row
// `is_cross_context: bool` (= `n_contexts > 1`) and `canonical_candidate`
// (= first crate in `crates[]` that matches any `:Context.canonical_crate`,
// or null) columns in Rust. The cypher-subset v0.1 RETURN clause does not
// evaluate comparison expressions or joined OPTIONAL MATCH outer-var
// equalities reliably (see `examples/queries/t1-concept-unwired.cypher`
// header for the evaluator limitations); the two derived columns therefore
// live in the trigger runner, not in this rule.
//
// # Why a SIBLING of `hsb-by-name.cypher`, not an in-place extension
//
// `hsb-by-name.cypher` has exactly one consumer today: `cfdb scope`
// (`crates/cfdb-cli/src/scope.rs:22`, embedded via `include_str!`), which
// projects rows into `ScopeInventory::canonical_candidates` reading the
// columns `name, kind, crates, qnames, files`. Adding `bounded_contexts` +
// `n_contexts` columns to hsb-by-name.cypher in place would be additive-
// compatible for that consumer (extra columns are ignored), but the cypher
// rule is a RFC-029 §13 Item 8 v0.1 locked-gate artifact — its RETURN
// shape is load-bearing for reviewers who read it standalone. T3 keeps the
// semantic separation explicit by landing as a sibling file.
//
// # Kind restriction — inherited from Pattern A doctrine
//
// `WHERE a.kind IN ['struct', 'enum', 'trait']` mirrors hsb-by-name.cypher's
// restriction. Fn/method name collisions across unrelated types are
// semantic noise (per hsb-by-name.cypher header lines 42-46); the
// refined fn/method homonym signal lives in
// `examples/queries/classifier-context-homonym.cypher` which uses the
// `signature_divergent` UDF from #47. T3 is the struct/enum/trait raw
// detector; the classifier handles fn/method via a different channel.
//
// # Test exemption
//
// `is_test=true` items under `#[cfg(test)]` are excluded — test helpers
// (`MockFoo`, `FixtureBar`) legitimately collide across crates.
//
// # Same-crate filter
//
// `count(DISTINCT a.crate) > 1` eliminates candidates where the same name
// appears across inner modules of one crate (legitimate reuse, not
// split-brain). `count(a) > 1` is redundant-but-cheap — kept for parity
// with hsb-by-name's defensive filter.
//
// # Determinism
//
// `ORDER BY n DESC, name ASC` — same sort as hsb-by-name.cypher; stable
// row order required by the dogfood determinism CI recipe.

MATCH (a:Item)
WHERE a.kind IN ['struct', 'enum', 'trait']
  AND a.is_test = false
WITH a.name AS name,
     a.kind AS kind,
     count(a) AS n,
     count(DISTINCT a.crate) AS n_crates,
     count(DISTINCT a.bounded_context) AS n_contexts,
     collect(DISTINCT a.crate) AS crates,
     collect(DISTINCT a.bounded_context) AS bounded_contexts,
     collect(a.qname) AS qnames,
     collect(a.file) AS files
WHERE n > 1
  AND n_crates > 1
RETURN name, kind, n, n_crates, n_contexts, crates, bounded_contexts, qnames, files
ORDER BY n DESC, name ASC

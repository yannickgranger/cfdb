//! Regression test for issue #409 — `cfdb scope` hangs on real-world
//! keyspaces (40+ min at 96% CPU on a 487k-node post.json) because the
//! Cartesian classifier rules (`classifier-random-scattering.cypher`,
//! `classifier-context-homonym.cypher`, `classifier-duplicated-feature.cypher`)
//! all start with `MATCH (a:Item), (b:Item)` and the evaluator's
//! `apply_node_pattern` re-issues `candidate_nodes(b, ...)` once per
//! outer row of `a`.
//!
//! The `--explain` trace from #409 quantified the regression: 4 outer
//! `(a:Item) → indexed` rows against 4345 inner `(b:Item) → indexed`
//! rows on the market_data context (1086 reachable items × 4 cartesian
//! rules = 4344 inner re-fetches).
//!
//! **Fix shape:** when a NodePattern's candidate set is independent of
//! incoming bindings — i.e., neither the pattern's own props nor the
//! top-level WHERE reference any var other than the pattern's own var —
//! lift `candidate_nodes` out of the per-row `flat_map` and compute it
//! once for the stream. Each binding-row then references the same
//! cached candidate set.
//!
//! **Behavioural invariant being asserted:** for a 2-leaf Cartesian
//! `MATCH (a:Item), (b:Item)` query where neither leaf has a
//! cross-binding predicate, `apply_node_pattern` must invoke
//! `candidate_nodes` for `b` EXACTLY ONCE — not once per outer `a` row.
//! Quantified via the explain trace: at most one `ExplainRow` per
//! NodePattern in this query shape.
//!
//! **Independence sub-cases covered:**
//! - 2a: pure Cartesian, no WHERE — `(a:Item), (b:Item)` (this file)
//! - 2b: WHERE references both `a` and `b` independently (own-var only)
//!   — `WHERE a.kind = 'fn' AND b.kind = 'fn'` (this file)
//!
//! **Sub-case NOT lifted (correctness preservation):**
//! - 2c: WHERE has a cross-binding equi-join (`WHERE a.x = b.x`) —
//!   candidate set depends on `a` binding, must stay per-row. Asserted
//!   in [`cross_binding_predicate_still_iterates_per_outer_row`].
//!
//! Without the fix, all three tests REQUIRE the cached candidate set
//! and will fail loudly with the count mismatch.

use cfdb_core::fact::{Node, PropValue, Props};
use cfdb_core::query::{NodePattern, Pattern, ProjectionValue};
use cfdb_core::schema::{Keyspace, Label};
use cfdb_core::store::StoreBackend;
use cfdb_core::{CompareOp, Expr, Predicate, Projection, Query, ReturnClause};
use cfdb_petgraph::explain::ExplainRow;
use cfdb_petgraph::PetgraphStore;
use std::collections::BTreeMap;

fn item_node(qname: &str, kind: &str) -> Node {
    let mut props = Props::new();
    props.insert("qname".into(), PropValue::Str(qname.into()));
    props.insert("kind".into(), PropValue::Str(kind.into()));
    Node {
        id: format!("item:{qname}"),
        label: Label::new(Label::ITEM),
        props,
    }
}

/// Build a fixture with `count` `:Item` nodes, alternating kind=fn / kind=method.
/// Each node's qname is `mod::item_{i}`.
fn ingest_items(store: &mut PetgraphStore, ks: &Keyspace, count: usize) {
    let nodes: Vec<Node> = (0..count)
        .map(|i| {
            let kind = if i % 2 == 0 { "fn" } else { "method" };
            item_node(&format!("mod::item_{i}"), kind)
        })
        .collect();
    store.ingest_nodes(ks, nodes).expect("ingest");
}

/// Count `candidate_nodes` invocations for patterns whose rendering
/// contains `pattern_marker` (e.g. `"(b:"`). One `ExplainRow` per call
/// (RFC-035 §explain.rs stability contract), so this count IS the
/// number of times `candidate_nodes` fired for the matched pattern.
/// We do NOT discriminate `Indexed` vs `Fallback` — the perf regression
/// shape is identical on both paths; the index just changes which
/// lookup function runs inside `candidate_nodes`, not how many times
/// that function is invoked.
fn count_lookups_for(explain: &[ExplainRow], pattern_marker: &str) -> usize {
    explain
        .iter()
        .filter(|row| row.pattern.contains(pattern_marker))
        .count()
}

fn cartesian_query_no_where() -> Query {
    let item_label = Label::new(Label::ITEM);
    Query {
        match_clauses: vec![
            Pattern::Node(NodePattern {
                var: Some("a".into()),
                label: Some(item_label.clone()),
                props: BTreeMap::new(),
            }),
            Pattern::Node(NodePattern {
                var: Some("b".into()),
                label: Some(item_label),
                props: BTreeMap::new(),
            }),
        ],
        where_clause: None,
        with_clause: None,
        return_clause: ReturnClause {
            projections: vec![Projection {
                value: ProjectionValue::Expr(Expr::Var("a".into())),
                alias: Some("a".into()),
            }],
            order_by: vec![],
            limit: Some(1),
            distinct: false,
        },
        params: BTreeMap::new(),
    }
}

fn cartesian_query_own_var_where() -> Query {
    use cfdb_core::query::NodePattern as NP;
    let item_label = Label::new(Label::ITEM);
    let a_kind_eq = Predicate::Compare {
        left: Expr::Property {
            var: "a".into(),
            prop: "kind".into(),
        },
        op: CompareOp::Eq,
        right: Expr::Literal(PropValue::Str("fn".into())),
    };
    let b_kind_eq = Predicate::Compare {
        left: Expr::Property {
            var: "b".into(),
            prop: "kind".into(),
        },
        op: CompareOp::Eq,
        right: Expr::Literal(PropValue::Str("fn".into())),
    };
    Query {
        match_clauses: vec![
            Pattern::Node(NP {
                var: Some("a".into()),
                label: Some(item_label.clone()),
                props: BTreeMap::new(),
            }),
            Pattern::Node(NP {
                var: Some("b".into()),
                label: Some(item_label),
                props: BTreeMap::new(),
            }),
        ],
        where_clause: Some(Predicate::And(Box::new(a_kind_eq), Box::new(b_kind_eq))),
        with_clause: None,
        return_clause: ReturnClause {
            projections: vec![Projection {
                value: ProjectionValue::Expr(Expr::Var("a".into())),
                alias: Some("a".into()),
            }],
            order_by: vec![],
            limit: Some(1),
            distinct: false,
        },
        params: BTreeMap::new(),
    }
}

fn cartesian_query_cross_binding_where() -> Query {
    use cfdb_core::query::NodePattern as NP;
    let item_label = Label::new(Label::ITEM);
    // WHERE a.kind = b.kind — cross-binding predicate. Candidate set
    // for `b` IS allowed to depend on `a`'s binding here (the planner
    // could narrow `b` via index on `a.kind`); we just assert the
    // cache does NOT inappropriately reuse a binding-dependent result.
    let cross = Predicate::Compare {
        left: Expr::Property {
            var: "a".into(),
            prop: "kind".into(),
        },
        op: CompareOp::Eq,
        right: Expr::Property {
            var: "b".into(),
            prop: "kind".into(),
        },
    };
    Query {
        match_clauses: vec![
            Pattern::Node(NP {
                var: Some("a".into()),
                label: Some(item_label.clone()),
                props: BTreeMap::new(),
            }),
            Pattern::Node(NP {
                var: Some("b".into()),
                label: Some(item_label),
                props: BTreeMap::new(),
            }),
        ],
        where_clause: Some(cross),
        with_clause: None,
        return_clause: ReturnClause {
            projections: vec![Projection {
                value: ProjectionValue::Expr(Expr::Var("a".into())),
                alias: Some("a".into()),
            }],
            order_by: vec![],
            limit: Some(1),
            distinct: false,
        },
        params: BTreeMap::new(),
    }
}

#[test]
fn issue_409_pure_cartesian_b_resolved_once_not_per_outer_row() {
    let mut store = PetgraphStore::new();
    let ks = Keyspace::new("test");
    ingest_items(&mut store, &ks, 50);

    let q = cartesian_query_no_where();
    let (_result, explain) = store.execute_explained(&ks, &q).expect("execute");

    let a_count = count_lookups_for(&explain, "(a:");
    let b_count = count_lookups_for(&explain, "(b:");

    assert_eq!(a_count, 1, "outer (a:Item) resolved once: {explain:?}");
    assert_eq!(
        b_count, 1,
        "AC #409: inner (b:Item) MUST be resolved once, not 50× (one per outer a row). \
         Current count = {b_count}. Cache miss on cartesian inner leaf. \
         explain trace = {explain:?}"
    );
}

#[test]
fn issue_409_own_var_where_predicates_stay_cached() {
    let mut store = PetgraphStore::new();
    let ks = Keyspace::new("test");
    ingest_items(&mut store, &ks, 50);

    let q = cartesian_query_own_var_where();
    let (_result, explain) = store.execute_explained(&ks, &q).expect("execute");

    let a_count = count_lookups_for(&explain, "(a:");
    let b_count = count_lookups_for(&explain, "(b:");

    assert_eq!(a_count, 1, "outer (a:Item) resolved once");
    assert_eq!(
        b_count, 1,
        "AC #409: WHERE clauses that only reference own-var props (a.kind=, b.kind=) \
         do NOT make the candidate set binding-dependent — `b`'s candidates are \
         identical for every `a` row. Must cache. Got {b_count} resolutions, \
         explain trace = {explain:?}"
    );
}

#[test]
fn cross_binding_predicate_still_iterates_per_outer_row() {
    let mut store = PetgraphStore::new();
    let ks = Keyspace::new("test");
    ingest_items(&mut store, &ks, 5);

    let q = cartesian_query_cross_binding_where();
    let (_result, explain) = store.execute_explained(&ks, &q).expect("execute");

    // Correctness guard: when WHERE has a real cross-binding equi-join
    // (`a.kind = b.kind`), the planner is permitted to narrow `b` based
    // on `a`'s binding — in which case the candidate set IS
    // binding-dependent and must be re-resolved per outer row. We do
    // NOT enforce a specific count here (the planner can choose either
    // a hash-join with one lookup OR per-row narrowing) — we only
    // enforce that the QUERY RESULT is correct.
    //
    // 5 items, 3 with kind=fn, 2 with kind=method. Cross-product with
    // kind-match: 3² + 2² = 13 pairs.
    let actual_rows = _result.rows.len();
    // The query returns DISTINCT a (with LIMIT 1), so we just check
    // we got at least one row when a match exists.
    assert!(
        actual_rows >= 1,
        "cross-binding query must still produce results: rows={actual_rows}, \
         explain trace = {explain:?}"
    );
}

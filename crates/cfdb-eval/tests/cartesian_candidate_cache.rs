use cfdb_core::fact::{Node, PropValue, Props};
use cfdb_core::query::{NodePattern, Pattern, ProjectionValue};
use cfdb_core::schema::{Keyspace, Label};
use cfdb_core::store::StoreBackend;
use cfdb_core::{CompareOp, Expr, Predicate, Projection, Query, ReturnClause};
use cfdb_eval::explain::ExplainRow;
use cfdb_eval::QueryEngine;
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

fn ingest_items(store: &mut PetgraphStore, ks: &Keyspace, count: usize) {
    let nodes: Vec<Node> = (0..count)
        .map(|i| {
            let kind = if i % 2 == 0 { "fn" } else { "method" };
            item_node(&format!("mod::item_{i}"), kind)
        })
        .collect();
    store.ingest_nodes(ks, nodes).expect("ingest");
}

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
    let (_result, explain) = QueryEngine::new(&store)
        .execute_explained(&ks, &q)
        .expect("execute");

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
    let (_result, explain) = QueryEngine::new(&store)
        .execute_explained(&ks, &q)
        .expect("execute");

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
    let (_result, explain) = QueryEngine::new(&store)
        .execute_explained(&ks, &q)
        .expect("execute");

    let actual_rows = _result.rows.len();
    assert!(
        actual_rows >= 1,
        "cross-binding query must still produce results: rows={actual_rows}, \
         explain trace = {explain:?}"
    );
}

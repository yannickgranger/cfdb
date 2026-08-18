//! Store-level query tests — `QueryEngine` over a `PetgraphStore`.
//!
//! Anchors: load the fixture and assert the spike-validated counts
//! (F1b=5, F2=20, F3=8), UnknownLabel warning path, OPTIONAL MATCH
//! null-fill, UNWIND, ORDER BY / LIMIT, var-length depth honouring, the
//! ingest-warning prepend on every result, unknown-keyspace error.

use std::collections::BTreeMap;

use cfdb_core::fact::{Edge, Node, PropValue};
use cfdb_core::query::{
    Aggregation, CompareOp, Direction, EdgePattern, Expr, NodePattern, ParamBinding, PathPattern,
    Pattern, Predicate, Projection, ProjectionValue, Query, ReturnClause, WithClause,
};
use cfdb_core::result::{RowValue, WarningKind};
use cfdb_core::schema::{EdgeLabel, Keyspace, Label};
use cfdb_core::store::{QueryBackend, StoreBackend};
use cfdb_eval::QueryEngine;
use cfdb_petgraph::PetgraphStore;

fn ks() -> Keyspace {
    Keyspace::new("test")
}

fn item(id: &str, qname: &str, krate: &str) -> Node {
    Node::new(id, Label::new(Label::ITEM))
        .with_prop("qname", qname)
        .with_prop("crate", krate)
}

fn call_site(id: &str) -> Node {
    Node::new(id, Label::new(Label::CALL_SITE))
}

#[test]
fn unresolved_edge_endpoint_warns_but_does_not_error() {
    let mut store = PetgraphStore::new();
    store
        .ingest_nodes(&ks(), vec![item("item:a", "foo::a", "c1")])
        .expect("ingest into fresh in-memory store never fails");
    store
        .ingest_edges(
            &ks(),
            vec![Edge::new(
                "item:a",
                "item:missing",
                EdgeLabel::new(EdgeLabel::CALLS),
            )],
        )
        .expect("ingest into fresh in-memory store never fails");

    // Run any query — the ingest warning should be surfaced on the result.
    let q = Query::new(
        vec![Pattern::Node(NodePattern {
            var: Some("a".into()),
            label: Some(Label::new(Label::ITEM)),
            props: BTreeMap::new(),
        })],
        ReturnClause {
            projections: vec![Projection {
                value: ProjectionValue::Expr(Expr::Var("a".into())),
                alias: Some("a".into()),
            }],
            order_by: vec![],
            limit: None,
            distinct: false,
        },
    );
    let result = QueryEngine::new(&store)
        .execute(&ks(), &q)
        .expect("fixture query executes against populated store");
    assert!(result
        .warnings
        .iter()
        .any(|w| w.message.contains("unknown dst id")));
}

#[test]
fn unknown_keyspace_returns_error() {
    let store = PetgraphStore::new();
    let q = Query::new(
        vec![Pattern::Node(NodePattern {
            var: Some("a".into()),
            label: None,
            props: BTreeMap::new(),
        })],
        ReturnClause {
            projections: vec![Projection {
                value: ProjectionValue::Expr(Expr::Var("a".into())),
                alias: None,
            }],
            order_by: vec![],
            limit: None,
            distinct: false,
        },
    );
    let err = QueryEngine::new(&store)
        .execute(&Keyspace::new("nope"), &q)
        .unwrap_err();
    assert!(matches!(
        err,
        cfdb_core::store::StoreError::UnknownKeyspace(_)
    ));
}

// ---- Fixture-driven tests: F1b=5, F2=20, F3=8 --------------------------

const FIXTURE_SMALL: &str = include_str!("../../../studies/spike/fixture-small.json");

#[derive(serde::Deserialize)]
struct FixtureNode {
    id: String,
    label: String,
    props: serde_json::Map<String, serde_json::Value>,
}

#[derive(serde::Deserialize)]
struct FixtureEdge {
    src: String,
    dst: String,
    label: String,
    #[serde(default)]
    #[allow(dead_code)]
    props: serde_json::Map<String, serde_json::Value>,
}

#[derive(serde::Deserialize)]
struct Fixture {
    nodes: Vec<FixtureNode>,
    edges: Vec<FixtureEdge>,
}

fn load_small_fixture(store: &mut PetgraphStore) {
    let fx: Fixture = serde_json::from_str(FIXTURE_SMALL).expect("fixture parses");
    let nodes: Vec<Node> = fx
        .nodes
        .into_iter()
        .map(|fn_| {
            let mut node = Node::new(fn_.id, Label::new(fn_.label));
            node.props.extend(
                fn_.props
                    .iter()
                    .map(|(k, v)| (k.clone(), PropValue::from_json(v))),
            );
            node
        })
        .collect();
    let edges: Vec<Edge> = fx
        .edges
        .into_iter()
        .map(|fe| Edge::new(fe.src, fe.dst, EdgeLabel::new(fe.label)))
        .collect();
    store
        .ingest_nodes(&ks(), nodes)
        .expect("ingest into fresh in-memory store never fails");
    store
        .ingest_edges(&ks(), edges)
        .expect("ingest into fresh in-memory store never fails");
}

/// Build the F1b query:
///   MATCH (a:Item)
///   WITH a.crate AS c, last_segment(a.qname) AS base
///   WITH base, count(DISTINCT c) AS n
///   WHERE n > 1
///   RETURN base
fn build_f1b_query() -> Query {
    use cfdb_core::query::ProjectionValue as PV;
    Query {
        match_clauses: vec![Pattern::Node(NodePattern {
            var: Some("a".into()),
            label: Some(Label::new(Label::ITEM)),
            props: BTreeMap::new(),
        })],
        where_clause: None,
        with_clause: Some(WithClause {
            projections: vec![
                Projection {
                    value: PV::Expr(Expr::Call {
                        name: "last_segment".into(),
                        args: vec![Expr::Property {
                            var: "a".into(),
                            prop: "qname".into(),
                        }],
                    }),
                    alias: Some("base".into()),
                },
                Projection {
                    value: PV::Aggregation(Aggregation::CountDistinct(Expr::Property {
                        var: "a".into(),
                        prop: "crate".into(),
                    })),
                    alias: Some("n".into()),
                },
            ],
            where_clause: Some(Predicate::Compare {
                left: Expr::Var("n".into()),
                op: CompareOp::Gt,
                right: Expr::Literal(PropValue::Int(1)),
            }),
        }),
        return_clause: ReturnClause {
            projections: vec![Projection {
                value: PV::Expr(Expr::Var("base".into())),
                alias: Some("base".into()),
            }],
            order_by: vec![],
            limit: None,
            distinct: false,
        },
        params: BTreeMap::new(),
    }
}

#[test]
fn f1b_aggregation_matches_spike_count() {
    let mut store = PetgraphStore::new();
    load_small_fixture(&mut store);

    let result = QueryEngine::new(&store)
        .execute(&ks(), &build_f1b_query())
        .expect("F1b query executes against loaded fixture");
    assert_eq!(
        result.rows.len(),
        5,
        "F1b must return 5 (spike-validated): got {}",
        result.rows.len()
    );
}

/// Build the F2 query:
///   MATCH (cs:CallSite)-[:CALLS*1..5]->(a:Item)
///   RETURN cs, a
fn build_f2_query() -> Query {
    Query {
        match_clauses: vec![Pattern::Path(PathPattern {
            from: NodePattern {
                var: Some("cs".into()),
                label: Some(Label::new(Label::CALL_SITE)),
                props: BTreeMap::new(),
            },
            edge: EdgePattern {
                var: None,
                label: Some(EdgeLabel::new(EdgeLabel::CALLS)),
                direction: Direction::Out,
                var_length: Some((1, 5)),
            },
            to: NodePattern {
                var: Some("a".into()),
                label: Some(Label::new(Label::ITEM)),
                props: BTreeMap::new(),
            },
        })],
        where_clause: None,
        with_clause: None,
        return_clause: ReturnClause {
            projections: vec![
                Projection {
                    value: ProjectionValue::Expr(Expr::Var("cs".into())),
                    alias: Some("cs".into()),
                },
                Projection {
                    value: ProjectionValue::Expr(Expr::Var("a".into())),
                    alias: Some("a".into()),
                },
            ],
            order_by: vec![],
            limit: None,
            distinct: false,
        },
        params: BTreeMap::new(),
    }
}

#[test]
fn f2_variable_length_matches_spike_count() {
    let mut store = PetgraphStore::new();
    load_small_fixture(&mut store);

    let result = QueryEngine::new(&store)
        .execute(&ks(), &build_f2_query())
        .expect("F2 query executes against loaded fixture");
    assert_eq!(
        result.rows.len(),
        20,
        "F2 must return 20 (spike-validated): got {}",
        result.rows.len()
    );
}

// --- var-length depth honouring -------------------
//
// `DEFAULT_VAR_LENGTH_MAX` (5) was silently clamping *every* var-length
// pattern, including explicit bounds (`*1..10` → 5). The fix honours
// explicit finite bounds as written, and treats the open form `*N..`
// (`u32::MAX`) as unbounded-via-visited-set. The fixture is an 8-hop linear
// chain — deeper than the old cap — so a silent clamp is observable.

/// Load a linear CALLS chain `cs -> f1 -> f2 -> ... -> f{hops}`: one CallSite
/// seed followed by `hops` Items, each calling the next. The chain length is
/// chosen `> DEFAULT_VAR_LENGTH_MAX` so the depth-5 clamp would truncate it.
fn load_linear_calls_chain(store: &mut PetgraphStore, hops: usize) {
    let mut nodes = vec![call_site("cs:0")];
    for i in 1..=hops {
        nodes.push(item(&format!("item:f{i}"), &format!("mod::f{i}"), "c1"));
    }
    let mut edges = vec![Edge::new(
        "cs:0",
        "item:f1",
        EdgeLabel::new(EdgeLabel::CALLS),
    )];
    for i in 1..hops {
        edges.push(Edge::new(
            format!("item:f{i}"),
            format!("item:f{}", i + 1),
            EdgeLabel::new(EdgeLabel::CALLS),
        ));
    }
    store
        .ingest_nodes(&ks(), nodes)
        .expect("ingest chain nodes into fresh store");
    store
        .ingest_edges(&ks(), edges)
        .expect("ingest chain edges into fresh store");
}

/// `MATCH (cs:CallSite)-[:CALLS*1..max]->(a:Item) RETURN a` — one row per
/// transitively-reached Item. `max == u32::MAX` is the open form `*1..`.
fn build_reach_query(max: u32) -> Query {
    Query {
        match_clauses: vec![Pattern::Path(PathPattern {
            from: NodePattern {
                var: Some("cs".into()),
                label: Some(Label::new(Label::CALL_SITE)),
                props: BTreeMap::new(),
            },
            edge: EdgePattern {
                var: None,
                label: Some(EdgeLabel::new(EdgeLabel::CALLS)),
                direction: Direction::Out,
                var_length: Some((1, max)),
            },
            to: NodePattern {
                var: Some("a".into()),
                label: Some(Label::new(Label::ITEM)),
                props: BTreeMap::new(),
            },
        })],
        where_clause: None,
        with_clause: None,
        return_clause: ReturnClause {
            projections: vec![Projection {
                value: ProjectionValue::Expr(Expr::Var("a".into())),
                alias: Some("a".into()),
            }],
            order_by: vec![],
            limit: None,
            distinct: false,
        },
        params: BTreeMap::new(),
    }
}

#[test]
fn var_length_honours_explicit_upper_bound_past_default_cap() {
    let mut store = PetgraphStore::new();
    load_linear_calls_chain(&mut store, 8);

    // `*1..10` over an 8-hop chain reaches all 8 Items — it must NOT clamp
    // to DEFAULT_VAR_LENGTH_MAX (5).
    let r10 = QueryEngine::new(&store)
        .execute(&ks(), &build_reach_query(10))
        .expect("*1..10 query executes");
    assert_eq!(
        r10.rows.len(),
        8,
        "*1..10 must honour the explicit bound and reach all 8 hops, not clamp to 5: got {}",
        r10.rows.len()
    );

    // A smaller explicit bound is honoured exactly: `*1..3` reaches 3 hops.
    let r3 = QueryEngine::new(&store)
        .execute(&ks(), &build_reach_query(3))
        .expect("*1..3 query executes");
    assert_eq!(
        r3.rows.len(),
        3,
        "*1..3 must reach exactly 3 hops: got {}",
        r3.rows.len()
    );
}

#[test]
fn var_length_open_form_is_unbounded_via_visited_set() {
    let mut store = PetgraphStore::new();
    load_linear_calls_chain(&mut store, 8);

    // The open form `*1..` (`u32::MAX`) traverses the full transitive set —
    // the visited-set is the only bound.
    let r_open = QueryEngine::new(&store)
        .execute(&ks(), &build_reach_query(u32::MAX))
        .expect("*1.. (open form) query executes");
    assert_eq!(
        r_open.rows.len(),
        8,
        "open form *1.. must reach all 8 hops unbounded: got {}",
        r_open.rows.len()
    );
}

/// Build the F3 query:
///   MATCH (a:Item) WHERE a.qname =~ '.*now_utc.*' RETURN a
fn build_f3_query() -> Query {
    Query {
        match_clauses: vec![Pattern::Node(NodePattern {
            var: Some("a".into()),
            label: Some(Label::new(Label::ITEM)),
            props: BTreeMap::new(),
        })],
        where_clause: Some(Predicate::Regex {
            left: Expr::Property {
                var: "a".into(),
                prop: "qname".into(),
            },
            pattern: Expr::Literal(PropValue::Str(".*now_utc.*".into())),
        }),
        with_clause: None,
        return_clause: ReturnClause {
            projections: vec![Projection {
                value: ProjectionValue::Expr(Expr::Property {
                    var: "a".into(),
                    prop: "qname".into(),
                }),
                alias: Some("qname".into()),
            }],
            order_by: vec![],
            limit: None,
            distinct: false,
        },
        params: BTreeMap::new(),
    }
}

#[test]
fn f3_regex_filter_matches_spike_count() {
    let mut store = PetgraphStore::new();
    load_small_fixture(&mut store);

    let result = QueryEngine::new(&store)
        .execute(&ks(), &build_f3_query())
        .expect("F3 query executes against loaded fixture");
    assert_eq!(
        result.rows.len(),
        8,
        "F3 must return 8 (spike-validated): got {}",
        result.rows.len()
    );
}

#[test]
fn unknown_label_emits_warning_with_suggestion() {
    let mut store = PetgraphStore::new();
    store
        .ingest_nodes(&ks(), vec![item("item:a", "foo::a", "c1")])
        .expect("ingest into fresh in-memory store never fails");

    let q = Query::new(
        vec![Pattern::Node(NodePattern {
            var: Some("a".into()),
            label: Some(Label::new("Ietm")), // typo for "Item"
            props: BTreeMap::new(),
        })],
        ReturnClause {
            projections: vec![Projection {
                value: ProjectionValue::Expr(Expr::Var("a".into())),
                alias: Some("a".into()),
            }],
            order_by: vec![],
            limit: None,
            distinct: false,
        },
    );
    let result = QueryEngine::new(&store)
        .execute(&ks(), &q)
        .expect("fixture query executes against populated store");
    assert!(result.rows.is_empty());
    let unknown = result
        .warnings
        .iter()
        .find(|w| matches!(w.kind, WarningKind::UnknownLabel))
        .expect("UnknownLabel warning must be emitted");
    assert!(
        unknown.suggestion.as_deref().unwrap_or("").contains("Item"),
        "did-you-mean should point at `Item`: got {:?}",
        unknown.suggestion
    );
}

#[test]
fn optional_match_null_fills_unmatched_bindings() {
    let mut store = PetgraphStore::new();
    store
        .ingest_nodes(
            &ks(),
            vec![
                item("item:a", "foo::a", "c1"),
                item("item:b", "foo::b", "c1"),
            ],
        )
        .expect("ingest into fresh in-memory store never fails");
    // No edges — OPTIONAL MATCH should null-fill.

    let q = Query {
        match_clauses: vec![
            Pattern::Node(NodePattern {
                var: Some("a".into()),
                label: Some(Label::new(Label::ITEM)),
                props: BTreeMap::new(),
            }),
            Pattern::Optional(Box::new(Pattern::Path(PathPattern {
                from: NodePattern {
                    var: Some("a".into()),
                    label: None,
                    props: BTreeMap::new(),
                },
                edge: EdgePattern {
                    var: None,
                    label: Some(EdgeLabel::new(EdgeLabel::CALLS)),
                    direction: Direction::Out,
                    var_length: None,
                },
                to: NodePattern {
                    var: Some("b".into()),
                    label: Some(Label::new(Label::ITEM)),
                    props: BTreeMap::new(),
                },
            }))),
        ],
        where_clause: None,
        with_clause: None,
        return_clause: ReturnClause {
            projections: vec![
                Projection {
                    value: ProjectionValue::Expr(Expr::Property {
                        var: "a".into(),
                        prop: "qname".into(),
                    }),
                    alias: Some("a_qname".into()),
                },
                Projection {
                    value: ProjectionValue::Expr(Expr::Var("b".into())),
                    alias: Some("b".into()),
                },
            ],
            order_by: vec![],
            limit: None,
            distinct: false,
        },
        params: BTreeMap::new(),
    };

    let result = QueryEngine::new(&store)
        .execute(&ks(), &q)
        .expect("fixture query executes against populated store");
    assert_eq!(result.rows.len(), 2, "two items, each null-filled for b");
    for row in &result.rows {
        assert_eq!(row.get("b"), Some(&RowValue::Scalar(PropValue::Null)));
    }
}

#[test]
fn unwind_list_param_cross_joins() {
    let mut store = PetgraphStore::new();
    store
        .ingest_nodes(&ks(), vec![item("item:a", "foo::a", "c1")])
        .expect("ingest into fresh in-memory store never fails");

    let mut params = BTreeMap::new();
    params.insert(
        "kinds".to_string(),
        ParamBinding::List(vec![
            PropValue::Str("fn".into()),
            PropValue::Str("struct".into()),
        ]),
    );

    let q = Query {
        match_clauses: vec![
            Pattern::Node(NodePattern {
                var: Some("a".into()),
                label: Some(Label::new(Label::ITEM)),
                props: BTreeMap::new(),
            }),
            Pattern::Unwind {
                list_param: "kinds".into(),
                var: "k".into(),
            },
        ],
        where_clause: None,
        with_clause: None,
        return_clause: ReturnClause {
            projections: vec![Projection {
                value: ProjectionValue::Expr(Expr::Var("k".into())),
                alias: Some("k".into()),
            }],
            order_by: vec![],
            limit: None,
            distinct: false,
        },
        params,
    };
    let result = QueryEngine::new(&store)
        .execute(&ks(), &q)
        .expect("fixture query executes against populated store");
    assert_eq!(result.rows.len(), 2);
}

#[test]
fn order_by_and_limit_are_applied() {
    let mut store = PetgraphStore::new();
    store
        .ingest_nodes(
            &ks(),
            vec![
                item("item:a", "foo::a", "c1"),
                item("item:b", "foo::b", "c1"),
                item("item:c", "foo::c", "c1"),
            ],
        )
        .expect("ingest into fresh in-memory store never fails");

    let q = Query {
        match_clauses: vec![Pattern::Node(NodePattern {
            var: Some("a".into()),
            label: Some(Label::new(Label::ITEM)),
            props: BTreeMap::new(),
        })],
        where_clause: None,
        with_clause: None,
        return_clause: ReturnClause {
            projections: vec![Projection {
                value: ProjectionValue::Expr(Expr::Property {
                    var: "a".into(),
                    prop: "qname".into(),
                }),
                alias: Some("qname".into()),
            }],
            order_by: vec![cfdb_core::query::OrderBy {
                expr: Expr::Var("qname".into()),
                descending: true,
            }],
            limit: Some(2),
            distinct: false,
        },
        params: BTreeMap::new(),
    };
    let result = QueryEngine::new(&store)
        .execute(&ks(), &q)
        .expect("fixture query executes against populated store");
    assert_eq!(result.rows.len(), 2);
    assert_eq!(
        result.rows[0].get("qname").and_then(|v| v.as_str()),
        Some("foo::c"),
        "DESC ORDER BY qname + LIMIT 2 should yield foo::c first"
    );
}

/// Regression: a bare `Var` reference in a RETURN projection
/// must surface a `RowValue::List` binding produced by a prior
/// `WITH collect(...)` aggregation. Before the fix at `eval.rs::apply_return`,
/// the non-aggregation RETURN path re-evaluated the `Var` through
/// `eval_expr` which only handles `Scalar` bindings and dropped Lists to
/// `null`. The enriched `hsb-by-name.cypher` rule
/// depends on this working — without it, `crates[]`, `qnames[]`, `files[]`
/// all come back null and the rule loses its entire triage signal.
#[test]
fn with_collect_then_return_var_preserves_list_binding_3675() {
    let mut store = PetgraphStore::new();
    store
        .ingest_nodes(
            &ks(),
            vec![
                Node::new("item:a", Label::new(Label::ITEM))
                    .with_prop("name", "OrderStatus")
                    .with_prop("crate", "domain-trading"),
                Node::new("item:b", Label::new(Label::ITEM))
                    .with_prop("name", "OrderStatus")
                    .with_prop("crate", "ports-trading"),
                Node::new("item:c", Label::new(Label::ITEM))
                    .with_prop("name", "PositionValuation")
                    .with_prop("crate", "domain-portfolio"),
            ],
        )
        .expect("ingest into fresh in-memory store never fails");

    // MATCH (a:Item) WITH a.name AS name, collect(a.crate) AS crates
    // WHERE count(*)-style is irrelevant here — we just care RETURN surfaces the list
    // RETURN name, crates
    let q = Query {
        match_clauses: vec![Pattern::Node(NodePattern {
            var: Some("a".into()),
            label: Some(Label::new(Label::ITEM)),
            props: BTreeMap::new(),
        })],
        where_clause: None,
        with_clause: Some(WithClause {
            projections: vec![
                Projection {
                    value: ProjectionValue::Expr(Expr::Property {
                        var: "a".into(),
                        prop: "name".into(),
                    }),
                    alias: Some("name".into()),
                },
                Projection {
                    value: ProjectionValue::Aggregation(Aggregation::Collect(Expr::Property {
                        var: "a".into(),
                        prop: "crate".into(),
                    })),
                    alias: Some("crates".into()),
                },
            ],
            where_clause: None,
        }),
        return_clause: ReturnClause {
            projections: vec![
                Projection {
                    value: ProjectionValue::Expr(Expr::Var("name".into())),
                    alias: Some("name".into()),
                },
                Projection {
                    value: ProjectionValue::Expr(Expr::Var("crates".into())),
                    alias: Some("crates".into()),
                },
            ],
            order_by: vec![],
            limit: None,
            distinct: false,
        },
        params: BTreeMap::new(),
    };

    let result = QueryEngine::new(&store)
        .execute(&ks(), &q)
        .expect("fixture query executes against populated store");

    // Find the OrderStatus row — the HSB candidate with 2 crates collected.
    let order_status_row = result
        .rows
        .iter()
        .find(|r| {
            matches!(
                r.get("name"),
                Some(RowValue::Scalar(PropValue::Str(s))) if s == "OrderStatus"
            )
        })
        .expect("OrderStatus row must be present");

    // The 'crates' column MUST be a List, not null. Pre-fix this would match
    // `RowValue::Scalar(PropValue::Null)` instead.
    let crates = order_status_row
        .get("crates")
        .expect("crates column must exist");
    match crates {
        RowValue::List(items) => {
            assert_eq!(items.len(), 2, "collect() must surface both crate values");
            let strs: Vec<&str> = items
                .iter()
                .filter_map(|p| match p {
                    PropValue::Str(s) => Some(s.as_str()),
                    _ => None,
                })
                .collect();
            assert!(strs.contains(&"domain-trading"));
            assert!(strs.contains(&"ports-trading"));
        }
        other => panic!("crates column should be RowValue::List, got {other:?}"),
    }
}

#[test]
fn count_star_aggregation() {
    let mut store = PetgraphStore::new();
    store
        .ingest_nodes(
            &ks(),
            vec![
                item("item:a", "foo::a", "c1"),
                item("item:b", "foo::b", "c1"),
                item("item:c", "foo::c", "c2"),
            ],
        )
        .expect("ingest into fresh in-memory store never fails");

    let q = Query::new(
        vec![Pattern::Node(NodePattern {
            var: Some("a".into()),
            label: Some(Label::new(Label::ITEM)),
            props: BTreeMap::new(),
        })],
        ReturnClause {
            projections: vec![Projection {
                value: ProjectionValue::Aggregation(Aggregation::CountStar),
                alias: Some("n".into()),
            }],
            order_by: vec![],
            limit: None,
            distinct: false,
        },
    );
    let result = QueryEngine::new(&store)
        .execute(&ks(), &q)
        .expect("fixture query executes against populated store");
    assert_eq!(result.rows.len(), 1);
    assert_eq!(
        result.rows[0].get("n"),
        Some(&RowValue::Scalar(PropValue::Int(3)))
    );
}

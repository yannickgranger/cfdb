//! Unit tests for `PetgraphStore`.
//!
//! Anchors: round-trip canonical dump, determinism, load the Gate 3 fixture
//! and assert the spike-validated counts (F1b=5, F2=20, F3=8), UnknownLabel
//! warning path, OPTIONAL MATCH null-fill.

use std::collections::BTreeMap;

use cfdb_core::fact::{Edge, Node, PropValue};
use cfdb_core::query::{
    Aggregation, CompareOp, Direction, EdgePattern, Expr, NodePattern, Param, PathPattern, Pattern,
    Predicate, Projection, ProjectionValue, Query, ReturnClause, WithClause,
};
use cfdb_core::result::{RowValue, WarningKind};
use cfdb_core::schema::{EdgeLabel, Keyspace, Label};
use cfdb_core::store::StoreBackend;

use crate::PetgraphStore;

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
fn ingest_round_trip_via_canonical_dump() {
    let mut store = PetgraphStore::new();
    let nodes = vec![
        item("item:a", "foo::bar", "c1"),
        item("item:b", "baz::bar", "c2"),
        call_site("cs:1"),
    ];
    let edges = vec![Edge::new(
        "cs:1",
        "item:a",
        EdgeLabel::new(EdgeLabel::CALLS),
    )];
    store
        .ingest_nodes(&ks(), nodes)
        .expect("ingest into fresh in-memory store never fails");
    store
        .ingest_edges(&ks(), edges)
        .expect("ingest into fresh in-memory store never fails");

    let dump = store
        .canonical_dump(&ks())
        .expect("canonical_dump over an ingested keyspace is infallible");
    assert!(dump.contains("item:a"));
    assert!(dump.contains("item:b"));
    assert!(dump.contains("cs:1"));
    assert!(dump.contains("CALLS"));
}

#[test]
fn canonical_dump_is_deterministic() {
    let mut store = PetgraphStore::new();
    let nodes: Vec<Node> = (0..20)
        .map(|i| item(&format!("item:n{}", i), &format!("mod::f{}", i), "c1"))
        .collect();
    let edges: Vec<Edge> = (0..19)
        .map(|i| {
            Edge::new(
                format!("item:n{}", i),
                format!("item:n{}", i + 1),
                EdgeLabel::new(EdgeLabel::CALLS),
            )
        })
        .collect();
    store
        .ingest_nodes(&ks(), nodes)
        .expect("ingest into fresh in-memory store never fails");
    store
        .ingest_edges(&ks(), edges)
        .expect("ingest into fresh in-memory store never fails");

    let d1 = store
        .canonical_dump(&ks())
        .expect("canonical_dump over an ingested keyspace is infallible");
    let d2 = store
        .canonical_dump(&ks())
        .expect("canonical_dump over an ingested keyspace is infallible");
    assert_eq!(
        d1.as_bytes(),
        d2.as_bytes(),
        "G1: canonical dump must be byte-identical across calls"
    );
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
    let result = store
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
    let err = store.execute(&Keyspace::new("nope"), &q).unwrap_err();
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
///
/// Spike simplification: group directly on `base` and count distinct crates.
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

    let result = store
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

    let result = store
        .execute(&ks(), &build_f2_query())
        .expect("F2 query executes against loaded fixture");
    assert_eq!(
        result.rows.len(),
        20,
        "F2 must return 20 (spike-validated): got {}",
        result.rows.len()
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

    let result = store
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
    let result = store
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

    let result = store
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
        Param::List(vec![
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
    let result = store
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
    let result = store
        .execute(&ks(), &q)
        .expect("fixture query executes against populated store");
    assert_eq!(result.rows.len(), 2);
    assert_eq!(
        result.rows[0].get("qname").and_then(|v| v.as_str()),
        Some("foo::c"),
        "DESC ORDER BY qname + LIMIT 2 should yield foo::c first"
    );
}

// ---- §12.1 sorted-jsonl canonical dump shape (#3630) -------------------

/// Helper: parse the canonical dump into a Vec of (raw_line, parsed_json).
/// Asserts every line is a valid JSON object — guards against the old
/// tab-prefixed `N\t...\t{json}` shape (#3630).
fn parse_dump_lines(dump: &str) -> Vec<(String, serde_json::Value)> {
    dump.lines()
        .map(|line| {
            let parsed: serde_json::Value = serde_json::from_str(line).unwrap_or_else(|e| {
                panic!("dump line is not pure JSON: {line:?}: {e}");
            });
            assert!(
                matches!(parsed, serde_json::Value::Object(_)),
                "dump line must be a JSON object, got: {line}"
            );
            (line.to_string(), parsed)
        })
        .collect()
}

#[test]
fn canonical_dump_lines_are_pure_jsonl() {
    let mut store = PetgraphStore::new();
    let nodes = vec![
        item("item:a", "foo::bar", "c1"),
        item("item:b", "baz::qux", "c2"),
    ];
    let edges = vec![Edge::new(
        "item:a",
        "item:b",
        EdgeLabel::new(EdgeLabel::CALLS),
    )];
    store
        .ingest_nodes(&ks(), nodes)
        .expect("ingest into fresh in-memory store never fails");
    store
        .ingest_edges(&ks(), edges)
        .expect("ingest into fresh in-memory store never fails");

    let dump = store
        .canonical_dump(&ks())
        .expect("canonical_dump over an ingested keyspace is infallible");
    // Must NOT use the old tab-prefix scheme.
    for line in dump.lines() {
        assert!(
            !line.starts_with("N\t"),
            "line uses old tab-prefix format: {line}"
        );
        assert!(
            !line.starts_with("E\t"),
            "line uses old tab-prefix format: {line}"
        );
    }
    // Every line must parse as a JSON object.
    let parsed = parse_dump_lines(&dump);
    assert_eq!(parsed.len(), 3, "2 nodes + 1 edge = 3 lines");
}

#[test]
fn canonical_dump_kind_discriminator_present() {
    let mut store = PetgraphStore::new();
    store
        .ingest_nodes(
            &ks(),
            vec![item("item:a", "foo::a", "c1"), call_site("cs:1")],
        )
        .expect("ingest into fresh in-memory store never fails");
    store
        .ingest_edges(
            &ks(),
            vec![Edge::new(
                "cs:1",
                "item:a",
                EdgeLabel::new(EdgeLabel::CALLS),
            )],
        )
        .expect("ingest into fresh in-memory store never fails");

    let dump = store
        .canonical_dump(&ks())
        .expect("canonical_dump over an ingested keyspace is infallible");
    let parsed = parse_dump_lines(&dump);

    let kinds: Vec<&str> = parsed
        .iter()
        .map(|(_, v)| v.get("kind").and_then(|k| k.as_str()).unwrap_or(""))
        .collect();
    assert_eq!(
        kinds.iter().filter(|k| **k == "node").count(),
        2,
        "expected 2 node lines: {kinds:?}"
    );
    assert_eq!(
        kinds.iter().filter(|k| **k == "edge").count(),
        1,
        "expected 1 edge line: {kinds:?}"
    );
}

#[test]
fn canonical_dump_field_order_is_alphabetical() {
    let mut store = PetgraphStore::new();
    store
        .ingest_nodes(&ks(), vec![item("item:a", "foo::bar", "c1")])
        .expect("ingest into fresh in-memory store never fails");

    let dump = store
        .canonical_dump(&ks())
        .expect("canonical_dump over an ingested keyspace is infallible");
    // The raw line text must show keys in strictly alphabetical order — checked
    // by extracting top-level keys via a regex-free positional walk.
    let line = dump.lines().next().expect("at least one line");
    let keys = top_level_json_keys(line);
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(
        keys, sorted,
        "top-level JSON keys are not alphabetical: {keys:?} (line: {line})"
    );
}

/// Helper: extract top-level JSON object keys by walking the byte stream.
/// Works because the canonical dump emits one object per line with no nested
/// embedded objects at the top level (props is the only nested object).
fn top_level_json_keys(line: &str) -> Vec<String> {
    let parsed: serde_json::Value = serde_json::from_str(line)
        .expect("caller has already validated line is pure JSON via parse_dump_lines");
    // serde_json's Map iterates in BTreeMap order by default; we re-derive by
    // re-serializing to bytes and walking the resulting string positionally.
    let bytes = serde_json::to_string(&parsed)
        .expect("re-serializing a just-parsed serde_json::Value is infallible");
    let mut keys = Vec::new();
    let mut chars = bytes.chars().peekable();
    let mut depth = 0i32;
    while let Some(c) = chars.next() {
        match c {
            '{' => depth += 1,
            '}' => depth -= 1,
            '"' if depth == 1 => {
                let mut k = String::new();
                while let Some(&ch) = chars.peek() {
                    chars.next();
                    if ch == '"' {
                        break;
                    }
                    k.push(ch);
                }
                // Only count if followed by ':' (key, not value)
                while let Some(&ch) = chars.peek() {
                    if ch == ':' {
                        keys.push(k);
                        break;
                    } else if !ch.is_whitespace() {
                        // it was a value, not a key
                        break;
                    }
                    chars.next();
                }
            }
            _ => {}
        }
    }
    keys
}

#[test]
fn canonical_dump_node_sort_uses_label_then_qname() {
    let mut store = PetgraphStore::new();
    // Two Item nodes: their qname order is baz::qux < foo::bar, so the sort
    // by (label, qname) places item:b BEFORE item:a even though id "item:a"
    // is lexicographically smaller.
    store
        .ingest_nodes(
            &ks(),
            vec![
                item("item:a", "foo::bar", "c1"),
                item("item:b", "baz::qux", "c2"),
            ],
        )
        .expect("ingest into fresh in-memory store never fails");

    let dump = store
        .canonical_dump(&ks())
        .expect("canonical_dump over an ingested keyspace is infallible");
    let parsed = parse_dump_lines(&dump);
    assert_eq!(parsed.len(), 2);
    let qnames: Vec<&str> = parsed
        .iter()
        .map(|(_, v)| {
            v.get("props")
                .and_then(|p| p.get("qname"))
                .and_then(|q| q.as_str())
                .unwrap_or("")
        })
        .collect();
    assert_eq!(
        qnames,
        vec!["baz::qux", "foo::bar"],
        "nodes must sort by (label, qname), not by id"
    );
}

#[test]
fn canonical_dump_edge_sort_uses_label_then_src_qname_then_dst_qname() {
    let mut store = PetgraphStore::new();
    // Three Item nodes whose qname order is alpha < bravo < charlie.
    store
        .ingest_nodes(
            &ks(),
            vec![
                item("item:a", "alpha", "c1"),
                item("item:b", "bravo", "c1"),
                item("item:c", "charlie", "c1"),
            ],
        )
        .expect("ingest into fresh in-memory store never fails");
    // Two CALLS edges with the same label but different src qnames:
    //   bravo -> charlie  and  alpha -> charlie
    // Sort order on (label, src_qname, dst_qname) places alpha->charlie first.
    store
        .ingest_edges(
            &ks(),
            vec![
                Edge::new("item:b", "item:c", EdgeLabel::new(EdgeLabel::CALLS)),
                Edge::new("item:a", "item:c", EdgeLabel::new(EdgeLabel::CALLS)),
            ],
        )
        .expect("ingest into fresh in-memory store never fails");

    let dump = store
        .canonical_dump(&ks())
        .expect("canonical_dump over an ingested keyspace is infallible");
    let parsed = parse_dump_lines(&dump);
    let edge_lines: Vec<&serde_json::Value> = parsed
        .iter()
        .filter_map(|(_, v)| {
            if v.get("kind").and_then(|k| k.as_str()) == Some("edge") {
                Some(v)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(edge_lines.len(), 2);

    let src_qnames: Vec<&str> = edge_lines
        .iter()
        .map(|v| v.get("src_qname").and_then(|s| s.as_str()).unwrap_or(""))
        .collect();
    assert_eq!(
        src_qnames,
        vec!["alpha", "bravo"],
        "edges must sort by (label, src_qname, dst_qname)"
    );
}

#[test]
fn canonical_dump_edge_sort_fallback_when_src_qname_absent() {
    let mut store = PetgraphStore::new();
    // CallSite nodes have no qname prop → sort key falls back to id.
    store
        .ingest_nodes(
            &ks(),
            vec![
                call_site("cs:zebra"),
                call_site("cs:apple"),
                item("item:target", "target_qname", "c1"),
            ],
        )
        .expect("ingest into fresh in-memory store never fails");
    store
        .ingest_edges(
            &ks(),
            vec![
                Edge::new("cs:zebra", "item:target", EdgeLabel::new(EdgeLabel::CALLS)),
                Edge::new("cs:apple", "item:target", EdgeLabel::new(EdgeLabel::CALLS)),
            ],
        )
        .expect("ingest into fresh in-memory store never fails");

    let dump = store
        .canonical_dump(&ks())
        .expect("canonical_dump over an ingested keyspace is infallible");
    let parsed = parse_dump_lines(&dump);
    let edge_lines: Vec<&serde_json::Value> = parsed
        .iter()
        .filter_map(|(_, v)| {
            if v.get("kind").and_then(|k| k.as_str()) == Some("edge") {
                Some(v)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(edge_lines.len(), 2);

    // Fallback: when src has no qname prop, src_qname = src.id ("cs:apple"
    // sorts before "cs:zebra").
    let src_qnames: Vec<&str> = edge_lines
        .iter()
        .map(|v| v.get("src_qname").and_then(|s| s.as_str()).unwrap_or(""))
        .collect();
    assert_eq!(
        src_qnames,
        vec!["cs:apple", "cs:zebra"],
        "fallback to id when qname prop is absent"
    );
}

#[test]
fn canonical_dump_lf_separated_no_trailing_newline() {
    let mut store = PetgraphStore::new();
    store
        .ingest_nodes(&ks(), vec![item("item:a", "foo::a", "c1")])
        .expect("ingest into fresh in-memory store never fails");

    let dump = store
        .canonical_dump(&ks())
        .expect("canonical_dump over an ingested keyspace is infallible");
    assert!(!dump.is_empty());
    assert!(
        !dump.ends_with('\n'),
        "canonical_dump output must NOT have a trailing newline (sha256sum reproducibility)"
    );
    // Must not contain CRLF.
    assert!(!dump.contains('\r'), "canonical_dump must use LF, not CRLF");
}

#[test]
fn canonical_dump_byte_identity_across_two_calls() {
    // This is the existing G1 invariant — must continue to hold under the
    // new format. Keeps the regression bar at parity with the old impl.
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
    store
        .ingest_edges(
            &ks(),
            vec![
                Edge::new("item:a", "item:b", EdgeLabel::new(EdgeLabel::CALLS)),
                Edge::new("item:b", "item:c", EdgeLabel::new(EdgeLabel::CALLS)),
            ],
        )
        .expect("ingest into fresh in-memory store never fails");

    let d1 = store
        .canonical_dump(&ks())
        .expect("canonical_dump over an ingested keyspace is infallible");
    let d2 = store
        .canonical_dump(&ks())
        .expect("canonical_dump over an ingested keyspace is infallible");
    assert_eq!(d1.as_bytes(), d2.as_bytes(), "G1 byte identity must hold");
}

/// Regression: issue #3675 — a bare `Var` reference in a RETURN projection
/// must surface a `RowValue::List` binding produced by a prior
/// `WITH collect(...)` aggregation. Before the fix at `eval.rs::apply_return`,
/// the non-aggregation RETURN path re-evaluated the `Var` through
/// `eval_expr` which only handles `Scalar` bindings and dropped Lists to
/// `null`. The enriched `hsb-by-name.cypher` rule (promoted to §13 Item 8)
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

    let result = store
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
    let result = store
        .execute(&ks(), &q)
        .expect("fixture query executes against populated store");
    assert_eq!(result.rows.len(), 1);
    assert_eq!(
        result.rows[0].get("n"),
        Some(&RowValue::Scalar(PropValue::Int(3)))
    );
}

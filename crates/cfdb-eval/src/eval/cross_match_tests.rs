use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

use cfdb_core::fact::{Node, PropValue};
use cfdb_core::query::{
    CompareOp, Expr, NodePattern, ParamBinding, Pattern, Predicate, Projection, ProjectionValue,
    Query, ReturnClause,
};
use cfdb_core::result::RowValue;
use cfdb_core::schema::{Keyspace, Label};
use cfdb_core::store::{QueryBackend, StoreBackend};
use cfdb_petgraph::index::spec::{ComputedKey, IndexEntry, IndexSpec};
use cfdb_petgraph::PetgraphStore;

use crate::QueryEngine;

const FIXTURE_SIZE: usize = 1_000;
const HOMONYM_PAIR_COUNT: usize = 10;

fn slice6_spec() -> IndexSpec {
    IndexSpec {
        entries: vec![
            IndexEntry::Prop {
                label: "Item".into(),
                prop: "qname".into(),
                notes: "slice-6 test".into(),
            },
            IndexEntry::Prop {
                label: "Item".into(),
                prop: "bounded_context".into(),
                notes: "slice-6 test".into(),
            },
            IndexEntry::Computed {
                label: "Item".into(),
                computed: ComputedKey::LastSegment,
                notes: "slice-6 test — homonym bucket key".into(),
            },
        ],
    }
}

fn build_fixture_nodes() -> Vec<Node> {
    let mut out: Vec<Node> = Vec::with_capacity(FIXTURE_SIZE);
    for i in 0..HOMONYM_PAIR_COUNT {
        let name = format!("shared_{i}");
        out.push(
            Node::new(format!("item:a:{i}"), Label::new("Item"))
                .with_prop("qname", format!("ctx_a::mod::{name}"))
                .with_prop("bounded_context", "A"),
        );
        out.push(
            Node::new(format!("item:b:{i}"), Label::new("Item"))
                .with_prop("qname", format!("ctx_b::mod::{name}"))
                .with_prop("bounded_context", "B"),
        );
    }
    let noise_start = 2 * HOMONYM_PAIR_COUNT;
    for i in noise_start..FIXTURE_SIZE {
        let ctx = if i % 3 == 0 { "A" } else { "B" };
        out.push(
            Node::new(format!("item:n:{i}"), Label::new("Item"))
                .with_prop(
                    "qname",
                    format!("ctx_{}::mod::uniq_{i}", ctx.to_lowercase()),
                )
                .with_prop("bounded_context", ctx),
        );
    }
    out
}

fn build_store(spec: IndexSpec) -> (PetgraphStore, Keyspace) {
    let ks = Keyspace::new("slice6-cross-match");
    let mut store = PetgraphStore::new().with_indexes(spec);
    store
        .ingest_nodes(&ks, build_fixture_nodes())
        .expect("ingest");
    (store, ks)
}

fn build_homonym_query(ctx: &str) -> Query {
    let props = BTreeMap::new();
    let a_np = NodePattern {
        var: Some("a".into()),
        label: Some(Label::new("Item")),
        props: props.clone(),
    };
    let b_np = NodePattern {
        var: Some("b".into()),
        label: Some(Label::new("Item")),
        props,
    };
    let call_last_segment = |var: &str| Expr::Call {
        name: "last_segment".into(),
        args: vec![Expr::Property {
            var: var.into(),
            prop: "qname".into(),
        }],
    };
    let ctx_eq = Predicate::Compare {
        left: Expr::Property {
            var: "a".into(),
            prop: "bounded_context".into(),
        },
        op: CompareOp::Eq,
        right: Expr::Param("ctx".into()),
    };
    let ctx_ne = Predicate::Compare {
        left: Expr::Property {
            var: "b".into(),
            prop: "bounded_context".into(),
        },
        op: CompareOp::Ne,
        right: Expr::Param("ctx".into()),
    };
    let last_seg_eq = Predicate::Compare {
        left: call_last_segment("a"),
        op: CompareOp::Eq,
        right: call_last_segment("b"),
    };
    let qname_ne = Predicate::Compare {
        left: Expr::Property {
            var: "a".into(),
            prop: "qname".into(),
        },
        op: CompareOp::Ne,
        right: Expr::Property {
            var: "b".into(),
            prop: "qname".into(),
        },
    };
    let where_clause = Predicate::And(
        Box::new(Predicate::And(Box::new(ctx_eq), Box::new(ctx_ne))),
        Box::new(Predicate::And(Box::new(last_seg_eq), Box::new(qname_ne))),
    );
    let mut params = BTreeMap::new();
    params.insert(
        "ctx".to_string(),
        ParamBinding::Scalar(PropValue::from(ctx)),
    );
    Query {
        match_clauses: vec![Pattern::Node(a_np), Pattern::Node(b_np)],
        where_clause: Some(where_clause),
        with_clause: None,
        return_clause: ReturnClause {
            projections: vec![
                Projection {
                    value: ProjectionValue::Expr(Expr::Property {
                        var: "a".into(),
                        prop: "qname".into(),
                    }),
                    alias: Some("aqn".into()),
                },
                Projection {
                    value: ProjectionValue::Expr(Expr::Property {
                        var: "b".into(),
                        prop: "qname".into(),
                    }),
                    alias: Some("bqn".into()),
                },
            ],
            order_by: vec![],
            limit: None,
            distinct: false,
        },
        params,
    }
}

fn collect_aqn(store: &PetgraphStore, ks: &Keyspace, ctx: &str) -> BTreeSet<String> {
    let query = build_homonym_query(ctx);
    let result = QueryEngine::new(store)
        .execute(ks, &query)
        .expect("execute");
    result
        .rows
        .into_iter()
        .filter_map(|row| {
            row.into_iter()
                .find_map(|(alias, value)| match (alias.as_str(), value) {
                    ("aqn", RowValue::Scalar(PropValue::Str(s))) => Some(s),
                    _ => None,
                })
        })
        .collect()
}

#[test]
fn cross_match_returns_exactly_ten_homonym_pairs_on_indexed_keyspace() {
    let (store, ks) = build_store(slice6_spec());
    let got = collect_aqn(&store, &ks, "A");
    assert_eq!(
        got.len(),
        HOMONYM_PAIR_COUNT,
        "indexed cross-MATCH must surface exactly {HOMONYM_PAIR_COUNT} a-side pairs"
    );
    for i in 0..HOMONYM_PAIR_COUNT {
        let expected = format!("ctx_a::mod::shared_{i}");
        assert!(
            got.contains(&expected),
            "missing expected a-qname in result: {expected}"
        );
    }
}

#[test]
fn cross_match_matches_full_scan_fallback_byte_for_byte() {
    let (indexed_store, indexed_ks) = build_store(slice6_spec());
    let (bare_store, bare_ks) = build_store(IndexSpec::empty());
    let via_index = collect_aqn(&indexed_store, &indexed_ks, "A");
    let via_scan = collect_aqn(&bare_store, &bare_ks, "A");
    assert_eq!(
        via_index, via_scan,
        "cross-MATCH fast path must be set-equivalent to the full-scan fallback"
    );
}

#[test]
fn cross_match_indexed_completes_under_100ms() {
    let (store, ks) = build_store(slice6_spec());
    let query = build_homonym_query("A");
    let engine = QueryEngine::new(&store);
    let _ = engine.execute(&ks, &query).expect("warm-up");
    let start = Instant::now();
    let _ = engine.execute(&ks, &query).expect("timed run");
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_millis() < 100,
        "indexed cross-MATCH exceeded 100 ms budget: {elapsed:?}"
    );
}

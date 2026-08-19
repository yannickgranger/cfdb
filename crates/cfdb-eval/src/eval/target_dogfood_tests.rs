use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Instant;

use cfdb_core::fact::PropValue;
use cfdb_core::query::{
    CompareOp, Expr, NodePattern, ParamBinding, Pattern, Predicate, Projection, ProjectionValue,
    Query, ReturnClause,
};
use cfdb_core::schema::{Keyspace, Label};
use cfdb_core::store::QueryBackend;
use cfdb_petgraph::index::spec::{ComputedKey, IndexEntry, IndexSpec};
use cfdb_petgraph::{persist, PetgraphStore};

use crate::QueryEngine;

const ENV_KEYSPACE: &str = "CFDB_TARGET_DOGFOOD_KEYSPACE";
const ENV_CONTEXT: &str = "CFDB_TARGET_DOGFOOD_CONTEXT";
const DEFAULT_CONTEXT: &str = "domain-trading";
const RFC035_WALL_BUDGET_SECS: u64 = 60;

fn slice6_spec() -> IndexSpec {
    IndexSpec {
        entries: vec![
            IndexEntry::Prop {
                label: "Item".into(),
                prop: "qname".into(),
                notes: "target-dogfood".into(),
            },
            IndexEntry::Prop {
                label: "Item".into(),
                prop: "bounded_context".into(),
                notes: "target-dogfood".into(),
            },
            IndexEntry::Computed {
                label: "Item".into(),
                computed: ComputedKey::LastSegment,
                notes: "target-dogfood — homonym bucket key".into(),
            },
        ],
    }
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

fn keyspace_path_from_env() -> Option<PathBuf> {
    match std::env::var(ENV_KEYSPACE) {
        Ok(s) if !s.is_empty() => {
            let p = PathBuf::from(s);
            if p.exists() {
                Some(p)
            } else {
                eprintln!(
                    "skip: {ENV_KEYSPACE} points to {} but file does not exist",
                    p.display()
                );
                None
            }
        }
        _ => {
            eprintln!("skip: set {ENV_KEYSPACE} to a qbot-core keyspace JSON to run this test");
            None
        }
    }
}

#[test]
#[ignore = "requires CFDB_TARGET_DOGFOOD_KEYSPACE pointing at an extracted qbot-core keyspace JSON"]
fn target_dogfood_homonym_completes_under_rfc035_wall_budget() {
    let Some(path) = keyspace_path_from_env() else {
        return;
    };
    let ctx = std::env::var(ENV_CONTEXT).unwrap_or_else(|_| DEFAULT_CONTEXT.to_string());

    let ks = Keyspace::new("qbot-core");
    let mut store = PetgraphStore::new().with_indexes(slice6_spec());

    let load_start = Instant::now();
    persist::load(&mut store, &ks, &path).expect("load keyspace");
    let load_elapsed = load_start.elapsed();
    eprintln!("load + index build wall: {load_elapsed:?}");

    let query = build_homonym_query(&ctx);

    let warm_start = Instant::now();
    let engine = QueryEngine::new(&store);
    let warm = engine.execute(&ks, &query).expect("warm-up execute");
    let warm_elapsed = warm_start.elapsed();
    eprintln!("warm-up wall: {warm_elapsed:?}, rows: {}", warm.rows.len());

    let timed_start = Instant::now();
    let timed = engine.execute(&ks, &query).expect("timed execute");
    let timed_elapsed = timed_start.elapsed();
    eprintln!(
        "timed wall: {timed_elapsed:?}, rows: {}, ctx={ctx}",
        timed.rows.len()
    );

    assert!(
        timed_elapsed.as_secs() < RFC035_WALL_BUDGET_SECS,
        "RFC-035 §9 wall-time budget violated: {timed_elapsed:?} >= {RFC035_WALL_BUDGET_SECS}s \
         (load {load_elapsed:?}, warm {warm_elapsed:?}, ctx={ctx})",
    );
}

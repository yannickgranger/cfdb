//! Target-dogfood measurement for RFC-035 slice 6 (#185) — runs the
//! `context_homonym`-shape cross-MATCH query against a real
//! qbot-core extraction and asserts the RFC §9 threshold of
//! **< 60 s wall time**.
//!
//! This test is the follow-up to PR #193, which shipped the slice-6
//! algorithm with the §9 measurement deferred. The deferral
//! rationale cited "IndexSpec must be wired through the CLI
//! composition root first (slice 7)". That rationale was wrong: the
//! CLI wiring only blocks running via `cfdb scope --context`. The
//! library-level evaluator is already reachable through
//! `KeyspaceState::new_with_spec` + `persist::load`, which is what
//! this test uses to drive the slice-6 code path at real scale.
//!
//! `#[ignore]` by default so CI does not depend on a 150k-node
//! extraction being on disk; the reviewer runs it explicitly via:
//!
//! ```bash
//! ./target/release/cfdb extract \
//!     --workspace /path/to/qbot-core \
//!     --db .proofs/target-185.db \
//!     --keyspace qbot-core
//!
//! CFDB_TARGET_DOGFOOD_KEYSPACE=.proofs/target-185.db/qbot-core.json \
//!   /usr/bin/time -v \
//!   cargo test --release -p cfdb-petgraph \
//!     eval::target_dogfood_tests -- --ignored --nocapture
//! ```
//!
//! Numbers captured by the author at qbot-core @ `6eb494ebe`
//! (148 959 nodes / 150 755 edges) on a 99%-CPU-bound single core
//! land in `.proofs/target-dogfood-185-followup.txt` and are
//! summarised in the PR body.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Instant;

use cfdb_core::fact::PropValue;
use cfdb_core::query::{
    CompareOp, Expr, NodePattern, Param, Pattern, Predicate, Projection, ProjectionValue, Query,
    ReturnClause,
};
use cfdb_core::schema::{Keyspace, Label};
use cfdb_core::store::StoreBackend;

use crate::graph::KeyspaceState;
use crate::index::spec::{ComputedKey, IndexEntry, IndexSpec};
use crate::{persist, PetgraphStore};

const ENV_KEYSPACE: &str = "CFDB_TARGET_DOGFOOD_KEYSPACE";
const ENV_CONTEXT: &str = "CFDB_TARGET_DOGFOOD_CONTEXT";
const DEFAULT_CONTEXT: &str = "domain-trading";
const RFC035_WALL_BUDGET_SECS: u64 = 60;

/// Slice-6 spec — `(Item, qname)`, `(Item, bounded_context)`, and
/// the computed `(Item, last_segment(qname))` bucket key. Duplicated
/// from `eval/cross_match_tests.rs` intentionally: this module is a
/// follow-up measurement surface and is committed to staying
/// independent of slice-6's pure-fixture suite.
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

/// `classifier-context-homonym.cypher`-shaped query (minus the
/// `signature_divergent` UDF which is not relevant to the index
/// mechanism being measured).
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
    params.insert("ctx".to_string(), Param::Scalar(PropValue::from(ctx)));
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

/// Skip-unless-env gate. Returns `Some(path)` if the extraction is
/// on disk at the configured location; `None` with a `eprintln!`
/// otherwise so `--ignored --nocapture` makes the skip reason
/// visible in the proof output.
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
    let mut store = PetgraphStore::new();
    store
        .keyspaces
        .insert(ks.clone(), KeyspaceState::new_with_spec(slice6_spec()));

    let load_start = Instant::now();
    persist::load(&mut store, &ks, &path).expect("load keyspace");
    let load_elapsed = load_start.elapsed();
    eprintln!("load + index build wall: {load_elapsed:?}");

    let query = build_homonym_query(&ctx);

    // Warm run absorbs first-touch lazy allocations so the timed
    // run measures steady-state evaluator cost, matching the
    // `cross_match_indexed_completes_under_100ms` convention.
    let warm_start = Instant::now();
    let warm = store.execute(&ks, &query).expect("warm-up execute");
    let warm_elapsed = warm_start.elapsed();
    eprintln!("warm-up wall: {warm_elapsed:?}, rows: {}", warm.rows.len());

    let timed_start = Instant::now();
    let timed = store.execute(&ks, &query).expect("timed execute");
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

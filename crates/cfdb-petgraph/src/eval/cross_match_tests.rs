//! Prescribed-test surface for RFC-035 slice 6 (#185) — the
//! `context_homonym`-shape cross-MATCH fixture.
//!
//! Builds a synthetic 1 000-node `:Item` keyspace with exactly 10
//! last-segment-colliding pairs across distinct bounded contexts
//! (matching the shape of `examples/queries/classifier-context-homonym.cypher`)
//! and asserts:
//!
//! 1. **Correctness** — the query returns exactly those 10 collision
//!    pairs (the rule surfaces the "a" side per `classifier-context-
//!    homonym.cypher`'s one-sided filter; we run the query with each
//!    context as `$context` and union the A sides to get all 10).
//! 2. **Equivalence** — the same query against an `IndexSpec::empty()`
//!    keyspace produces the same result set. The index is a pure
//!    narrowing optimisation; no node is gained or lost.
//! 3. **Wall time** — on the indexed keyspace the query completes in
//!    under 100 ms (prescribed upper bound in the #185 body).
//!
//! Lives in a sibling `#[cfg(test)] mod` declared from `eval/mod.rs`
//! so it can reach `pub(super)` / `pub(crate)` items without
//! widening the public surface.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

use cfdb_core::fact::{Node, PropValue};
use cfdb_core::query::{
    CompareOp, Expr, NodePattern, Param, Pattern, Predicate, Projection, ProjectionValue, Query,
    ReturnClause,
};
use cfdb_core::result::RowValue;
use cfdb_core::schema::{Keyspace, Label};
use cfdb_core::store::StoreBackend;

use crate::graph::KeyspaceState;
use crate::index::spec::{ComputedKey, IndexEntry, IndexSpec};
use crate::PetgraphStore;

const FIXTURE_SIZE: usize = 1_000;
const HOMONYM_PAIR_COUNT: usize = 10;

/// Spec with `(Item, qname)`, `(Item, bounded_context)`, and the
/// `(Item, last_segment(qname))` computed index that slice-6
/// cross-MATCH intersection consumes.
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

/// Build `FIXTURE_SIZE` `:Item` nodes. The first `2 *
/// HOMONYM_PAIR_COUNT` are arranged as pairs `(i, i +
/// HOMONYM_PAIR_COUNT)` that share a last-segment but live in
/// distinct bounded contexts — these are the homonyms the rule
/// must find. All others have unique qnames and the same context
/// as their row-index mod 3 dictates, chosen to be noise.
fn build_fixture_nodes() -> Vec<Node> {
    let mut out: Vec<Node> = Vec::with_capacity(FIXTURE_SIZE);
    // Pairs: 2*HOMONYM_PAIR_COUNT slots (0..20) carrying ctx=A/B and
    // the same `shared_N` last-segment.
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
    // Noise: unique qnames, mixed contexts. Start from 2 *
    // HOMONYM_PAIR_COUNT so IDs don't collide with pair slots.
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
    let mut store = PetgraphStore::new();
    store
        .keyspaces
        .insert(ks.clone(), KeyspaceState::new_with_spec(spec));
    store
        .ingest_nodes(&ks, build_fixture_nodes())
        .expect("ingest");
    (store, ks)
}

/// Build the context_homonym-shape Query — simplified shape of
/// `examples/queries/classifier-context-homonym.cypher` without the
/// `signature_divergent` UDF. That UDF is hard-wired in the
/// evaluator and not indexable here; the joinable predicate
/// (`last_segment(a.qname) = last_segment(b.qname)`) is what slice
/// 6 targets.
///
/// ```text
/// MATCH (a:Item), (b:Item)
/// WHERE a.bounded_context = $ctx
///   AND b.bounded_context <> $ctx
///   AND last_segment(a.qname) = last_segment(b.qname)
///   AND a.qname <> b.qname
/// RETURN a.qname AS aqn, b.qname AS bqn
/// ```
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

/// Collect the set of `a.qname` values returned by the homonym
/// query. Each row's `aqn` column is a `Str`; we lift it out for
/// comparison.
fn collect_aqn(store: &PetgraphStore, ks: &Keyspace, ctx: &str) -> BTreeSet<String> {
    let query = build_homonym_query(ctx);
    let result = store.execute(ks, &query).expect("execute");
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
    // Running with ctx=A surfaces all the `item:a:*` side of each
    // of the 10 pairs.
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
    // Warm the executor with one throwaway call; the first run
    // includes one-time lazy allocations.
    let _ = store.execute(&ks, &query).expect("warm-up");
    let start = Instant::now();
    let _ = store.execute(&ks, &query).expect("timed run");
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_millis() < 100,
        "indexed cross-MATCH exceeded 100 ms budget: {elapsed:?}"
    );
}

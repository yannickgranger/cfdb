//! Impact seed-binding composition — RFC-047a slice 47-0 (#488).
//!
//! ## Why this test exists, and why there is no `src/` change for it
//!
//! Slice 47-0 was originally filed to "land list-valued param binding so
//! `IN $seeds` has a path" for the forthcoming `cfdb impact` verb. The
//! council's premise — *"no list-binding path exists today"* — was false
//! (RFC-047a §1): the in-process path `impact` uses — `parse(template)` →
//! `query.params.insert(name, Param::List(..))` → `store.execute` — already
//! ships and runs in production (`check-predicate` #147, the `list:` param
//! form #145, the raid-plan suite #205). `cfdb_core::Param::List` exists; the
//! evaluator already resolves a list param for `IN`. So there is no
//! production code to write *for the binding*.
//!
//! What was NOT pinned anywhere is the EXACT shape `impact` composes: a
//! variable-length REVERSE traversal `(seed)<-[:CALLS*1..]-(affected)`
//! FILTERED by `WHERE seed.qname IN $seeds` bound as a `Param::List`. Both
//! halves were tested independently; their composition was not. This test
//! locks that composition. It depends on RFC-047a's B1 (the open form `*1..`
//! parses) and B2 (the traversal is not silently clamped) — the two `src/`
//! changes this slice ships.
//!
//! Scope note: the CALLS-graph *self-dogfood* (impact over cfdb's own
//! extracted call graph) is **slice 47-A's** — it needs the HIR extraction
//! path to produce resolved `Item→Item CALLS` edges (the syn-based
//! `extract_workspace` emits only `INVOKES_AT`; RFC-047a §3.3). This file
//! stays on a fact-injected fixture, isolating the query shape from extractor
//! stability (the same pattern as `raid_plan_queries.rs`).

use std::collections::{BTreeMap, BTreeSet};

use cfdb_core::fact::{Edge, Node, PropValue};
use cfdb_core::qname::item_node_id;
use cfdb_core::schema::{EdgeLabel, Keyspace, Label};
use cfdb_core::store::StoreBackend;
use cfdb_petgraph::PetgraphStore;
use cfdb_query::impact_query;

// The canonical `cfdb impact` reverse-reachability query (RFC-047 §3.2) and
// its `$seeds` `Param::List` binding are owned by `cfdb_query::impact_query`
// (slice 47-A / #489) — this test executes that composer's output against a
// fact-injected fixture, so the canonical shape is defined once.

// ---------------------------------------------------------------------
// Fixture builders — a minimal synthetic `CALLS` graph. Fact injection
// (not extraction) isolates the query shape from extractor stability,
// the same pattern as `raid_plan_queries.rs`.
// ---------------------------------------------------------------------

fn item_node(qname: &str) -> Node {
    let mut props = BTreeMap::new();
    props.insert("qname".into(), PropValue::Str(qname.into()));
    props.insert("kind".into(), PropValue::Str("fn".into()));
    Node {
        id: item_node_id(qname),
        label: Label::new(Label::ITEM),
        props,
    }
}

fn calls_edge(caller_qname: &str, callee_qname: &str) -> Edge {
    let mut props = BTreeMap::new();
    props.insert("resolved".into(), PropValue::Bool(true));
    Edge {
        src: item_node_id(caller_qname),
        dst: item_node_id(callee_qname),
        label: EdgeLabel::new(EdgeLabel::CALLS),
        props,
    }
}

/// Two independent call chains plus one unconnected island:
///
/// ```text
///   top_x ── CALLS ──▶ mid_1 ── CALLS ──▶ leaf_a
///                      mid_2 ── CALLS ──▶ leaf_b
///   island            (no edges)
/// ```
///
/// Reverse-reachable callers: `leaf_a` ← {mid_1, top_x}; `leaf_b` ← {mid_2}.
fn fixture() -> (PetgraphStore, Keyspace) {
    let ks = Keyspace::new("impact-47-0");
    let nodes = vec![
        item_node("imp::leaf_a"),
        item_node("imp::leaf_b"),
        item_node("imp::mid_1"),
        item_node("imp::mid_2"),
        item_node("imp::top_x"),
        item_node("imp::island"),
    ];
    let edges = vec![
        calls_edge("imp::mid_1", "imp::leaf_a"),
        calls_edge("imp::top_x", "imp::mid_1"),
        calls_edge("imp::mid_2", "imp::leaf_b"),
    ];
    let mut store = PetgraphStore::new();
    store
        .ingest_nodes(&ks, nodes)
        .expect("ingest fixture nodes");
    store
        .ingest_edges(&ks, edges)
        .expect("ingest fixture edges");
    (store, ks)
}

/// Run the impact query (built by `cfdb_query::impact_query`, which binds
/// `$seeds` as a `Param::List`) and collect the affected qnames as a set.
fn affected_qnames(store: &PetgraphStore, ks: &Keyspace, seeds: &[&str]) -> BTreeSet<String> {
    let query = impact_query(seeds);
    store
        .execute(ks, &query)
        .expect("execute impact query")
        .rows
        .iter()
        .filter_map(|row| {
            row.get("qname")
                .and_then(|v| v.as_str())
                .map(str::to_string)
        })
        .collect()
}

fn string_set<'a>(items: impl IntoIterator<Item = &'a str>) -> BTreeSet<String> {
    items.into_iter().map(str::to_string).collect()
}

// ---------------------------------------------------------------------
// Unit (fixture) — the composed reverse-var-length + IN-$list shape.
// ---------------------------------------------------------------------

#[test]
fn reverse_calls_with_list_seeds_returns_caller_union() {
    let (store, ks) = fixture();

    // Two seeds bound as one `Param::List` → the union of each seed's
    // transitive callers. `leaf_a` is called by `mid_1` (depth 1), itself
    // called by `top_x` (depth 2); `leaf_b` is called by `mid_2` (depth 1).
    let affected = affected_qnames(&store, &ks, &["imp::leaf_a", "imp::leaf_b"]);

    assert_eq!(
        affected,
        string_set(["imp::mid_1", "imp::top_x", "imp::mid_2"]),
        "two-seed `IN $seeds` must return the union of both seeds' transitive callers"
    );
    assert!(
        !affected.contains("imp::island"),
        "an item with no CALLS path to a seed must not appear in the blast radius"
    );
}

#[test]
fn single_seed_proves_list_membership_filters() {
    let (store, ks) = fixture();

    // Binding only `leaf_a` must exclude `leaf_b`'s caller (`mid_2`) —
    // proving the `IN $seeds` predicate filters by list membership, not
    // "every caller regardless of seed".
    let affected = affected_qnames(&store, &ks, &["imp::leaf_a"]);

    assert_eq!(
        affected,
        string_set(["imp::mid_1", "imp::top_x"]),
        "single-seed affected set must be exactly `leaf_a`'s transitive callers"
    );
    assert!(
        !affected.contains("imp::mid_2"),
        "`leaf_b`'s caller must be absent when `leaf_b` is not in $seeds"
    );
}

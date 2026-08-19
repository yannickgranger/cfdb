use std::collections::{BTreeMap, BTreeSet};

use cfdb_core::fact::{Edge, Node, PropValue};
use cfdb_core::qname::item_node_id;
use cfdb_core::schema::{EdgeLabel, Keyspace, Label};
use cfdb_core::store::{QueryBackend, StoreBackend};
use cfdb_eval::QueryEngine;
use cfdb_petgraph::PetgraphStore;
use cfdb_query::impact_query;

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

fn affected_qnames(store: &PetgraphStore, ks: &Keyspace, seeds: &[&str]) -> BTreeSet<String> {
    let query = impact_query(seeds, None);
    QueryEngine::new(store)
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

#[test]
fn reverse_calls_with_list_seeds_returns_caller_union() {
    let (store, ks) = fixture();

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

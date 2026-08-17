//! Delegation tests for `GraphView`/`GraphBackend` (RFC-056 slice 056-0).
//!
//! Additive-only — no enrichment pass moved yet. Every assertion here
//! compares the port method's result against the equivalent direct
//! `KeyspaceState`/`PetgraphStore` call, on a small synthetic fixture, to
//! pin that the port is a pure delegation with no behavior of its own.

use cfdb_core::fact::{Edge, Node, Props};
use cfdb_core::graph::GraphBackend;
use cfdb_core::schema::{Direction, EdgeLabel, Keyspace, Label};
use cfdb_core::store::StoreBackend;

use crate::PetgraphStore;

fn label(s: &str) -> Label {
    Label::from(s)
}

fn edge_label(s: &str) -> EdgeLabel {
    EdgeLabel::from(s)
}

fn node(id: &str, lbl: &str) -> Node {
    Node {
        id: id.to_string(),
        label: label(lbl),
        props: Props::new(),
    }
}

/// A small fixture: two `:Fn` nodes and one `:Item` node, with the `:Item`
/// having two outgoing `CALLS` edges (a multi-edge-label node — RFC-056
/// §7's 056-0 Tests block asks for at least one).
fn fixture() -> (PetgraphStore, Keyspace) {
    let ks = Keyspace::new("test");
    let mut store = PetgraphStore::new();
    store
        .ingest_nodes(
            &ks,
            vec![node("a", "Item"), node("b", "Fn"), node("c", "Fn")],
        )
        .expect("ingest nodes");
    store
        .ingest_edges(
            &ks,
            vec![
                Edge {
                    src: "a".into(),
                    dst: "b".into(),
                    label: edge_label("CALLS"),
                    props: Props::new(),
                },
                Edge {
                    src: "a".into(),
                    dst: "c".into(),
                    label: edge_label("USES"),
                    props: Props::new(),
                },
            ],
        )
        .expect("ingest edges");
    (store, ks)
}

#[test]
fn node_by_id_matches_direct_lookup() {
    let (mut store, ks) = fixture();
    let view = store.graph_view(&ks).expect("known keyspace");
    assert_eq!(view.node_by_id("a").map(|n| n.id.as_str()), Some("a"));
    assert_eq!(view.node_by_id("nope"), None);
}

#[test]
fn nodes_with_label_matches_sorted_ids() {
    let (mut store, ks) = fixture();
    let view = store.graph_view(&ks).expect("known keyspace");
    let mut fn_ids = view.nodes_with_label(&label("Fn"));
    fn_ids.sort();
    assert_eq!(fn_ids, vec!["b".to_string(), "c".to_string()]);
}

#[test]
fn neighbors_outgoing_returns_both_edge_labels() {
    let (mut store, ks) = fixture();
    let view = store.graph_view(&ks).expect("known keyspace");
    let mut neighbors = view.neighbors("a", Direction::Out);
    neighbors.sort();
    assert_eq!(
        neighbors,
        vec![
            (edge_label("CALLS"), "b".to_string()),
            (edge_label("USES"), "c".to_string()),
        ]
    );
}

#[test]
fn neighbors_incoming_from_the_far_endpoint() {
    let (mut store, ks) = fixture();
    let view = store.graph_view(&ks).expect("known keyspace");
    assert_eq!(
        view.neighbors("b", Direction::In),
        vec![(edge_label("CALLS"), "a".to_string())]
    );
}

#[test]
fn neighbors_undirected_unions_both_directions() {
    let (mut store, ks) = fixture();
    let view = store.graph_view(&ks).expect("known keyspace");
    let mut neighbors = view.neighbors("a", Direction::Undirected);
    neighbors.sort();
    assert_eq!(
        neighbors,
        vec![
            (edge_label("CALLS"), "b".to_string()),
            (edge_label("USES"), "c".to_string()),
        ]
    );
}

#[test]
fn set_attr_writes_through_to_node_by_id() {
    let (mut store, ks) = fixture();
    let view = store.graph_view(&ks).expect("known keyspace");
    assert!(view.set_attr(
        "a",
        "bounded_context",
        cfdb_core::fact::PropValue::Str("x".into())
    ));
    assert_eq!(
        view.node_by_id("a")
            .and_then(|n| n.props.get("bounded_context"))
            .and_then(cfdb_core::fact::PropValue::as_str),
        Some("x")
    );
    assert!(!view.set_attr("nope", "k", cfdb_core::fact::PropValue::Str("v".into())));
}

#[test]
fn ingest_nodes_and_edges_are_visible_through_the_view() {
    let (mut store, ks) = fixture();
    let view = store.graph_view(&ks).expect("known keyspace");
    view.ingest_nodes(vec![node("d", "Item")]);
    assert!(view.node_by_id("d").is_some());
    view.ingest_edges(vec![Edge {
        src: "a".into(),
        dst: "d".into(),
        label: edge_label("USES"),
        props: Props::new(),
    }]);
    assert!(view
        .neighbors("a", Direction::Out)
        .contains(&(edge_label("USES"), "d".to_string())));
}

#[test]
fn graph_view_unknown_keyspace_returns_err() {
    let mut store = PetgraphStore::new();
    let ks = Keyspace::new("never");
    let Err(err) = store.graph_view(&ks) else {
        panic!("unknown keyspace must err");
    };
    assert!(format!("{err:?}").contains("UnknownKeyspace"));
}

#[test]
fn workspace_root_matches_direct_accessor() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let store = PetgraphStore::new().with_workspace(tmp.path());
    assert_eq!(
        GraphBackend::workspace_root(&store),
        PetgraphStore::workspace_root(&store)
    );
}

fn _assert_send_sync<T: Send + Sync>() {}

#[test]
fn petgraph_store_is_send_sync_as_a_graph_backend() {
    _assert_send_sync::<PetgraphStore>();
}

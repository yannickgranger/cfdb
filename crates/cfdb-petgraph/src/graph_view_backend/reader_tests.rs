//! Delegation tests for `GraphReader` on `KeyspaceState` and
//! `GraphBackend::graph_reader` on `PetgraphStore`.
//!
//! Every assertion compares the port method's result against the
//! equivalent direct `KeyspaceState` accessor or raw-field read on one
//! synthetic fixture, so the port is pinned as pure delegation with no
//! behavior of its own. This file is the ONLY place a handle's raw value is
//! compared against a petgraph index — the port's consumers never do.

use std::collections::BTreeMap;

use petgraph::stable_graph::NodeIndex;
use petgraph::visit::EdgeRef;
use petgraph::Direction as PetDirection;

use cfdb_core::fact::{Edge, Node, PropValue, Props};
use cfdb_core::graph::{GraphBackend, GraphReader, NodeHandle};
use cfdb_core::query::{CompareOp, Expr, NodePattern, ParamBinding, Predicate};
use cfdb_core::schema::{EdgeLabel, Keyspace, Label};
use cfdb_core::store::{QueryBackend, StoreBackend};

use crate::graph::KeyspaceState;
use crate::index::build::index_key_of;
use crate::index::lookup::candidates_from_index;
use crate::index::spec::{IndexEntry, IndexSpec};
use crate::PetgraphStore;

fn qname_spec() -> IndexSpec {
    IndexSpec {
        entries: vec![IndexEntry::Prop {
            label: "Item".into(),
            prop: "qname".into(),
            notes: "test".into(),
        }],
    }
}

fn item(id: &str, qname: &str) -> Node {
    Node::new(id, Label::new("Item")).with_prop("qname", qname)
}

fn fn_node(id: &str) -> Node {
    Node::new(id, Label::new("Fn"))
}

fn edge(src: &str, dst: &str, label: &str) -> Edge {
    Edge {
        src: src.into(),
        dst: dst.into(),
        label: EdgeLabel::from(label),
        props: Props::new(),
    }
}

/// Three `:Item` nodes (two sharing a qname, all indexed on `qname`), two
/// `:Fn` nodes, a multi-edge-label node (`i1` → `f1` CALLS, `i1` → `f2`
/// USES), an incoming edge on `f1` from `i2` too, and one edge whose `dst`
/// is unknown so the keyspace records an ingest warning.
fn fixture() -> (PetgraphStore, Keyspace) {
    let ks = Keyspace::new("test");
    let mut store = PetgraphStore::new().with_indexes(qname_spec());
    store
        .ingest_nodes(
            &ks,
            vec![
                item("i1", "foo::a"),
                item("i2", "foo::b"),
                item("i3", "foo::a"),
                fn_node("f1"),
                fn_node("f2"),
            ],
        )
        .expect("ingest nodes");
    store
        .ingest_edges(
            &ks,
            vec![
                edge("i1", "f1", "CALLS"),
                edge("i1", "f2", "USES"),
                edge("i2", "f1", "CALLS"),
                edge("i3", "nope", "CALLS"),
            ],
        )
        .expect("ingest edges");
    (store, ks)
}

fn state<'s>(store: &'s PetgraphStore, ks: &Keyspace) -> &'s KeyspaceState {
    store.keyspaces.get(ks).expect("known keyspace")
}

fn raw_of(idx: NodeIndex) -> u32 {
    idx.index() as u32
}

#[test]
fn nodes_with_label_is_the_direct_accessor_in_the_same_order() {
    let (store, ks) = fixture();
    let st = state(&store, &ks);
    let expected: Vec<u32> = KeyspaceState::nodes_with_label(st, &Label::new("Item"))
        .into_iter()
        .map(raw_of)
        .collect();
    let got: Vec<u32> = GraphReader::nodes_with_label(st, &Label::new("Item"))
        .into_iter()
        .map(NodeHandle::raw)
        .collect();
    assert_eq!(got, expected);
    assert_eq!(got.len(), 3);
    assert!(
        got.windows(2).all(|w| w[0] < w[1]),
        "handle order is index order"
    );
}

#[test]
fn all_nodes_sorted_is_the_direct_accessor_in_the_same_order() {
    let (store, ks) = fixture();
    let st = state(&store, &ks);
    let expected: Vec<u32> = KeyspaceState::all_nodes_sorted(st)
        .into_iter()
        .map(raw_of)
        .collect();
    let got: Vec<u32> = GraphReader::all_nodes_sorted(st)
        .into_iter()
        .map(NodeHandle::raw)
        .collect();
    assert_eq!(got, expected);
    assert_eq!(got.len(), 5);
}

#[test]
fn vocabulary_probes_match_the_raw_indexes() {
    let (store, ks) = fixture();
    let st = state(&store, &ks);
    let reader: &dyn GraphReader = st;
    assert!(reader.has_label(&Label::new("Item")));
    assert!(reader.has_label(&Label::new("Fn")));
    assert!(!reader.has_label(&Label::new("Nope")));
    assert_eq!(
        reader.labels(),
        st.by_label.keys().cloned().collect::<Vec<_>>()
    );
    assert!(reader.has_edge_label(&EdgeLabel::from("CALLS")));
    assert!(!reader.has_edge_label(&EdgeLabel::from("NOPE")));
    assert_eq!(
        reader.edge_labels(),
        st.edge_labels.iter().cloned().collect::<Vec<_>>()
    );
    assert_eq!(reader.edge_labels().len(), 2);
}

#[test]
fn node_and_edge_dereference_the_underlying_weights() {
    let (store, ks) = fixture();
    let st = state(&store, &ks);
    let reader: &dyn GraphReader = st;
    for h in reader.all_nodes_sorted() {
        let idx = NodeIndex::new(h.raw() as usize);
        assert_eq!(
            reader.node(h).map(|n| &n.id),
            st.graph.node_weight(idx).map(|n| &n.id)
        );
    }
    let i1 = *st.id_to_idx.get("i1").expect("i1");
    let (eh, _) = reader.edges_out(NodeHandle::from_raw(raw_of(i1)))[0];
    let expected = st
        .graph
        .edges_directed(i1, PetDirection::Outgoing)
        .next()
        .map(|e| e.weight().label.clone());
    assert_eq!(reader.edge(eh).map(|e| e.label.clone()), expected);
    assert!(reader.node(NodeHandle::from_raw(u32::MAX)).is_none());
}

#[test]
fn adjacency_matches_edges_directed_in_the_same_order() {
    let (store, ks) = fixture();
    let st = state(&store, &ks);
    let reader: &dyn GraphReader = st;
    let i1 = *st.id_to_idx.get("i1").expect("i1");
    let f1 = *st.id_to_idx.get("f1").expect("f1");

    let out_expected: Vec<(u32, u32)> = st
        .graph
        .edges_directed(i1, PetDirection::Outgoing)
        .map(|e| (e.id().index() as u32, raw_of(e.target())))
        .collect();
    let out_got: Vec<(u32, u32)> = reader
        .edges_out(NodeHandle::from_raw(raw_of(i1)))
        .into_iter()
        .map(|(e, n)| (e.raw(), n.raw()))
        .collect();
    assert_eq!(out_got, out_expected);
    assert_eq!(out_got.len(), 2, "i1 has two outgoing edges (CALLS + USES)");

    let in_expected: Vec<(u32, u32)> = st
        .graph
        .edges_directed(f1, PetDirection::Incoming)
        .map(|e| (e.id().index() as u32, raw_of(e.source())))
        .collect();
    let in_got: Vec<(u32, u32)> = reader
        .edges_in(NodeHandle::from_raw(raw_of(f1)))
        .into_iter()
        .map(|(e, n)| (e.raw(), n.raw()))
        .collect();
    assert_eq!(in_got, in_expected);
    assert_eq!(in_got.len(), 2, "f1 is called from i1 and i2");
    assert!(reader
        .edges_out(NodeHandle::from_raw(raw_of(f1)))
        .is_empty());
}

fn np_item(qname: Option<&str>) -> NodePattern {
    let mut props = BTreeMap::new();
    if let Some(q) = qname {
        props.insert("qname".to_string(), PropValue::from(q));
    }
    NodePattern {
        var: Some("a".into()),
        label: Some(Label::new("Item")),
        props,
    }
}

#[test]
fn index_candidates_matches_candidates_from_index() {
    let (store, ks) = fixture();
    let st = state(&store, &ks);
    let reader: &dyn GraphReader = st;
    let params: BTreeMap<String, ParamBinding> = BTreeMap::new();
    let no_bound_pv = |_: &str, _: &str| -> Option<PropValue> { None };
    let no_bound_key = |_: &str, _: &str| -> Option<String> { None };

    let np = np_item(Some("foo::a"));
    let expected: Option<Vec<u32>> = candidates_from_index(st, &np, None, &params, &no_bound_key)
        .map(|v| v.into_iter().map(raw_of).collect());
    let got: Option<Vec<u32>> = reader
        .index_candidates(&np, None, &params, &no_bound_pv)
        .map(|v| v.into_iter().map(NodeHandle::raw).collect());
    assert_eq!(got, expected);
    assert_eq!(
        got.as_ref().map(Vec::len),
        Some(2),
        "two :Item share qname foo::a"
    );

    let empty = np_item(Some("does::not::exist"));
    assert_eq!(
        reader.index_candidates(&empty, None, &params, &no_bound_pv),
        Some(vec![])
    );

    let unhinted = np_item(None);
    assert_eq!(
        reader.index_candidates(&unhinted, None, &params, &no_bound_pv),
        None
    );
}

#[test]
fn index_candidates_routes_bound_var_values_through_index_key_of() {
    let (store, ks) = fixture();
    let st = state(&store, &ks);
    let reader: &dyn GraphReader = st;
    let params: BTreeMap<String, ParamBinding> = BTreeMap::new();
    let bound_pv = |var: &str, prop: &str| -> Option<PropValue> {
        (var == "b" && prop == "qname").then(|| PropValue::from("foo::b"))
    };
    let bound_key = |var: &str, prop: &str| -> Option<String> {
        bound_pv(var, prop).and_then(|pv| index_key_of(&pv))
    };
    let np = np_item(None);
    let pred = Predicate::Compare {
        left: Expr::Property {
            var: "a".into(),
            prop: "qname".into(),
        },
        op: CompareOp::Eq,
        right: Expr::Property {
            var: "b".into(),
            prop: "qname".into(),
        },
    };
    let expected: Option<Vec<u32>> =
        candidates_from_index(st, &np, Some(&pred), &params, &bound_key)
            .map(|v| v.into_iter().map(raw_of).collect());
    let got: Option<Vec<u32>> = reader
        .index_candidates(&np, Some(&pred), &params, &bound_pv)
        .map(|v| v.into_iter().map(NodeHandle::raw).collect());
    assert_eq!(got, expected);
    assert_eq!(
        got.as_ref().map(Vec::len),
        Some(1),
        "only i2 carries foo::b"
    );
}

#[test]
fn indexed_prop_is_populated_matches_the_by_prop_probe() {
    let (store, ks) = fixture();
    let st = state(&store, &ks);
    let reader: &dyn GraphReader = st;
    let item = Label::new("Item");
    let probe = |tag: &str| {
        st.by_prop
            .get(&(item.clone(), tag.to_string()))
            .is_some_and(|bucket| !bucket.is_empty())
    };
    assert!(reader.indexed_prop_is_populated(&item, "qname"));
    assert_eq!(
        reader.indexed_prop_is_populated(&item, "qname"),
        probe("qname")
    );
    assert!(!reader.indexed_prop_is_populated(&item, "bounded_context"));
    assert_eq!(
        reader.indexed_prop_is_populated(&item, "bounded_context"),
        probe("bounded_context")
    );
    assert!(!reader.indexed_prop_is_populated(&Label::new("Fn"), "qname"));
}

#[test]
fn ingest_warnings_are_the_materialized_set() {
    let (store, ks) = fixture();
    let st = state(&store, &ks);
    let reader: &dyn GraphReader = st;
    let expected = st.materialized_ingest_warnings();
    assert!(
        !expected.is_empty(),
        "the unknown-dst edge records a warning"
    );
    assert_eq!(reader.ingest_warnings(), expected);
    assert_eq!(reader.ingest_warnings(), store.ingest_warnings(&ks));
}

#[test]
fn graph_reader_resolves_known_keyspace_and_rejects_unknown() {
    let (store, ks) = fixture();
    let reader = store.graph_reader(&ks).expect("known keyspace");
    assert_eq!(reader.all_nodes_sorted().len(), 5);
    let err = store
        .graph_reader(&Keyspace::new("missing"))
        .err()
        .expect("unknown keyspace is an error");
    assert!(format!("{err:?}").contains("UnknownKeyspace"));
}

fn _takes_dyn_query_backend(_: &dyn QueryBackend) {}

fn _takes_dyn_graph_reader(_: &dyn GraphReader) {}

#[test]
fn query_backend_and_graph_reader_are_object_safe() {
    let (store, ks) = fixture();
    _takes_dyn_graph_reader(store.graph_reader(&ks).expect("known keyspace"));
}

use std::collections::{BTreeMap, BTreeSet};

use cfdb_core::fact::{Node, PropValue};
use cfdb_core::graph::{GraphBackend, GraphReader, NodeHandle};
use cfdb_core::query::{CompareOp, Expr, NodePattern, ParamBinding, Predicate};
use cfdb_core::schema::{Keyspace, Label};
use cfdb_core::store::StoreBackend;
use cfdb_petgraph::index::spec::{IndexEntry, IndexSpec};
use cfdb_petgraph::PetgraphStore;

use super::Evaluator;

const FIXTURE_SIZE: usize = 1_000;

fn qname_indexed_spec() -> IndexSpec {
    IndexSpec {
        entries: vec![IndexEntry::Prop {
            label: "Item".into(),
            prop: "qname".into(),
            notes: "test".into(),
        }],
    }
}

fn build_fixture_nodes() -> Vec<Node> {
    (0..FIXTURE_SIZE)
        .map(|i| {
            Node::new(format!("item:{i}"), Label::new("Item"))
                .with_prop("qname", format!("item::{i}"))
                .with_prop("name", if i % 3 == 0 { "triple" } else { "other" })
        })
        .collect()
}

fn build_state(spec: IndexSpec) -> (PetgraphStore, Keyspace) {
    let ks = Keyspace::new("fast-path");
    let mut store = PetgraphStore::new().with_indexes(spec);
    store
        .ingest_nodes(&ks, build_fixture_nodes())
        .expect("ingest fixture");
    (store, ks)
}

fn reader((store, ks): &(PetgraphStore, Keyspace)) -> &dyn GraphReader {
    store.graph_reader(ks).expect("known keyspace")
}

fn pattern_with_prop(prop: &str, value: &str) -> NodePattern {
    let mut props = BTreeMap::new();
    props.insert(prop.to_string(), PropValue::from(value));
    NodePattern {
        var: Some("a".into()),
        label: Some(Label::new("Item")),
        props,
    }
}

fn pattern_bare_label() -> NodePattern {
    NodePattern {
        var: Some("a".into()),
        label: Some(Label::new("Item")),
        props: BTreeMap::new(),
    }
}

fn where_eq(prop: &str, value: &str) -> Predicate {
    Predicate::Compare {
        left: Expr::Property {
            var: "a".into(),
            prop: prop.into(),
        },
        op: CompareOp::Eq,
        right: Expr::Literal(PropValue::from(value)),
    }
}

fn final_set(
    state: &dyn GraphReader,
    np: &NodePattern,
    where_clause: Option<&Predicate>,
) -> BTreeSet<NodeHandle> {
    let params: BTreeMap<String, ParamBinding> = BTreeMap::new();
    let eval = Evaluator::new(state, &params);
    let empty_bindings = crate::eval::Bindings::new();
    eval.candidate_nodes(np, where_clause, &empty_bindings)
        .into_iter()
        .filter(|idx| eval.node_props_match(*idx, np))
        .collect()
}

#[test]
fn label_plus_literal_fast_path_matches_full_scan() {
    let indexed = build_state(qname_indexed_spec());
    let bare = build_state(IndexSpec::empty());
    let np = pattern_with_prop("qname", "item::42");

    let via_index = final_set(reader(&indexed), &np, None);
    let via_scan = final_set(reader(&bare), &np, None);

    assert_eq!(
        via_index, via_scan,
        "fast path 1 (label + pattern literal) must equal full scan + post-filter"
    );
    assert_eq!(via_index.len(), 1, "exactly one node matches item::42");
}

#[test]
fn label_plus_where_eq_fast_path_matches_full_scan() {
    let indexed = build_state(qname_indexed_spec());
    let bare = build_state(IndexSpec::empty());
    let np = pattern_bare_label();
    let pred = where_eq("qname", "item::17");

    let via_index = final_set(reader(&indexed), &np, Some(&pred));
    let via_scan = final_set(reader(&bare), &np, Some(&pred));

    let params: BTreeMap<String, ParamBinding> = BTreeMap::new();
    let eval_indexed = Evaluator::new(reader(&indexed), &params);
    let eval_bare = Evaluator::new(reader(&bare), &params);
    let via_index_filtered: BTreeSet<NodeHandle> = via_index
        .into_iter()
        .filter(|idx| {
            let row = one_row_with_a(*idx);
            eval_indexed.eval_predicate(&pred, &row)
        })
        .collect();
    let via_scan_filtered: BTreeSet<NodeHandle> = via_scan
        .into_iter()
        .filter(|idx| {
            let row = one_row_with_a(*idx);
            eval_bare.eval_predicate(&pred, &row)
        })
        .collect();

    assert_eq!(
        via_index_filtered, via_scan_filtered,
        "fast path 2 (label + WHERE Eq) must equal full scan + WHERE filter"
    );
    assert_eq!(via_index_filtered.len(), 1);
}

#[test]
fn non_indexed_prop_falls_back_to_label_scan() {
    let indexed = build_state(qname_indexed_spec());
    let bare = build_state(IndexSpec::empty());
    let np = pattern_with_prop("name", "triple");

    let via_index = final_set(reader(&indexed), &np, None);
    let via_scan = final_set(reader(&bare), &np, None);

    assert_eq!(
        via_index, via_scan,
        "non-indexed prop must fall back to label scan; both sides equal"
    );
    let expected = (0..FIXTURE_SIZE).filter(|i| i % 3 == 0).count();
    assert_eq!(
        via_index.len(),
        expected,
        "every third fixture node has name='triple'"
    );
}

fn one_row_with_a(h: NodeHandle) -> crate::eval::Bindings {
    let mut row = crate::eval::Bindings::new();
    row.insert("a".into(), crate::eval::Binding::NodeRef(h));
    row
}

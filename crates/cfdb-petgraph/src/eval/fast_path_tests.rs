//! Prescribed-test surface for RFC-035 slice 5 (#184) — asserts
//! `candidate_nodes` returns the same `BTreeSet<NodeIndex>` with the
//! index fast paths enabled and with a bare `IndexSpec` (fallback to
//! the full `by_label` scan).
//!
//! Three shapes are covered per the issue body:
//!
//! - **(a) Label + pattern literal** — `MATCH (a:Item {qname: "…"})`.
//! - **(b) Label + WHERE Eq on literal** — `MATCH (a:Item) WHERE a.qname = "…"`.
//! - **(c) Non-indexed prop fallback** — pattern on a prop NOT in the
//!   spec; both states return the full label set and the evaluator's
//!   `emit_anon_node` / `emit_new_var_node` filter narrows afterwards.
//!
//! The equality test is: filter both sides through the same
//! [`Evaluator::node_props_match`] post-filter (which is what the
//! caller `apply_node_pattern` does) and assert the resulting
//! `BTreeSet<NodeIndex>` is identical. That captures the contract
//! "indexed lookup is equivalent to full scan + prop filter".
//!
//! A sibling `#[cfg(test)] mod` sits under `eval/` so these tests
//! reach `pub(super)` items (`Evaluator::candidate_nodes`,
//! `Evaluator::node_props_match`) without opening a crate-wide
//! accessor surface.

use std::collections::{BTreeMap, BTreeSet};

use cfdb_core::fact::{Node, PropValue};
use cfdb_core::query::{CompareOp, Expr, NodePattern, Param, Predicate};
use cfdb_core::schema::Label;
use petgraph::stable_graph::NodeIndex;

use super::Evaluator;
use crate::graph::KeyspaceState;
use crate::index::spec::{IndexEntry, IndexSpec};

const FIXTURE_SIZE: usize = 1_000;

/// Spec that indexes `(Item, qname)` — required for fast paths 1 & 2
/// on the prescribed test surface.
fn qname_indexed_spec() -> IndexSpec {
    IndexSpec {
        entries: vec![IndexEntry::Prop {
            label: "Item".into(),
            prop: "qname".into(),
            notes: "test".into(),
        }],
    }
}

/// Deterministic fixture: `FIXTURE_SIZE` `:Item` nodes with distinct
/// qnames `item::0` .. `item::999` and a secondary `name` prop
/// (non-indexed) that duplicates across buckets so the fallback test
/// can observe more-than-one match.
fn build_fixture_nodes() -> Vec<Node> {
    (0..FIXTURE_SIZE)
        .map(|i| {
            Node::new(format!("item:{i}"), Label::new("Item"))
                .with_prop("qname", format!("item::{i}"))
                .with_prop("name", if i % 3 == 0 { "triple" } else { "other" })
        })
        .collect()
}

fn build_state(spec: IndexSpec) -> KeyspaceState {
    let mut state = KeyspaceState::new_with_spec(spec);
    state.ingest_nodes(build_fixture_nodes());
    state
}

/// Build a [`NodePattern`] with variable `a`, label `Item`, and an
/// inline prop map `{prop: value}`.
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

/// Convert `candidate_nodes` output into a `BTreeSet` post-filtered
/// by `node_props_match`, which is what `apply_node_pattern` does
/// before yielding bindings. The post-filter makes the set
/// comparable across fast-path and full-scan code paths: the fast
/// path narrows already; the full scan narrows via the post-filter.
fn final_set(
    state: &KeyspaceState,
    np: &NodePattern,
    where_clause: Option<&Predicate>,
) -> BTreeSet<NodeIndex> {
    let params: BTreeMap<String, Param> = BTreeMap::new();
    let eval = Evaluator::new(state, &params);
    // Slice-5 surface: no incoming bindings (single-MATCH). Slice-6
    // cross-MATCH lookups pass the per-row bindings instead.
    let empty_bindings = crate::eval::Bindings::new();
    eval.candidate_nodes(np, where_clause, &empty_bindings)
        .into_iter()
        .filter(|idx| eval.node_props_match(*idx, np))
        .collect()
}

/// Case (a) — `MATCH (a:Item {qname: "item::42"})`. The single
/// matching node must be returned by both the fast path and the
/// full-scan fallback; the two sets must be byte-identical.
#[test]
fn label_plus_literal_fast_path_matches_full_scan() {
    let indexed = build_state(qname_indexed_spec());
    let bare = build_state(IndexSpec::empty());
    let np = pattern_with_prop("qname", "item::42");

    let via_index = final_set(&indexed, &np, None);
    let via_scan = final_set(&bare, &np, None);

    assert_eq!(
        via_index, via_scan,
        "fast path 1 (label + pattern literal) must equal full scan + post-filter"
    );
    assert_eq!(via_index.len(), 1, "exactly one node matches item::42");
}

/// Case (b) — `MATCH (a:Item) WHERE a.qname = "item::17"`. The
/// WHERE Eq must thread through `candidate_nodes` and produce the
/// same narrow set as the full-scan fallback.
#[test]
fn label_plus_where_eq_fast_path_matches_full_scan() {
    let indexed = build_state(qname_indexed_spec());
    let bare = build_state(IndexSpec::empty());
    let np = pattern_bare_label();
    let pred = where_eq("qname", "item::17");

    let via_index = final_set(&indexed, &np, Some(&pred));
    let via_scan = final_set(&bare, &np, Some(&pred));

    // The fast path narrows to one node; the full scan returns all
    // 1000 but `node_props_match` on a bare-label pattern matches
    // every node because `np.props` is empty. WHERE applies outside
    // `candidate_nodes` — so the fast-path side is the ONLY side
    // actually constrained by the WHERE at the candidate stage.
    //
    // To make the comparison meaningful we post-filter BOTH sides
    // through the WHERE predicate (which is what `Evaluator::run`
    // does after all pattern stages).
    let params: BTreeMap<String, Param> = BTreeMap::new();
    let eval_indexed = Evaluator::new(&indexed, &params);
    let eval_bare = Evaluator::new(&bare, &params);
    let via_index_filtered: BTreeSet<NodeIndex> = via_index
        .into_iter()
        .filter(|idx| {
            let row = one_row_with_a(*idx);
            eval_indexed.eval_predicate(&pred, &row)
        })
        .collect();
    let via_scan_filtered: BTreeSet<NodeIndex> = via_scan
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

/// Case (c) — `MATCH (a:Item {name: "triple"})`. `name` is not in
/// the spec; both paths must return the full 334-node set (every
/// 3rd node). The fast path returns `None` for "no indexable hint";
/// the full-scan path returns all 1000 label matches; both sides
/// end up equal after `node_props_match` post-filters.
#[test]
fn non_indexed_prop_falls_back_to_label_scan() {
    let indexed = build_state(qname_indexed_spec());
    let bare = build_state(IndexSpec::empty());
    let np = pattern_with_prop("name", "triple");

    let via_index = final_set(&indexed, &np, None);
    let via_scan = final_set(&bare, &np, None);

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

/// Helper: manufacture a one-entry `Bindings` row with `a ->
/// NodeRef(idx)` so we can reuse `Evaluator::eval_predicate` to
/// post-filter a candidate set by the WHERE predicate.
fn one_row_with_a(idx: NodeIndex) -> crate::eval::Bindings {
    let mut row = crate::eval::Bindings::new();
    row.insert("a".into(), crate::eval::Binding::NodeRef(idx));
    row
}

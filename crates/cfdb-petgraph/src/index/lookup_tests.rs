//! Unit tests for [`crate::index::lookup`] (RFC-035 slices 5 #184 + 6 #185).
//!
//! Lives in a sibling `#[cfg(test)] mod` declared from
//! `index/mod.rs` so `lookup.rs` stays under the workspace god-file
//! ceiling (500 LoC). Tests reach `pub(crate)` `candidates_from_index`
//! via `use super::lookup::*` — the helpers it exercises (private
//! `collect_pattern_hints`, `collect_where_hints`, …) are tested
//! transitively through the public entry point; direct-private-fn
//! tests added no coverage the entry-point path didn't already cover.

use std::collections::BTreeMap;

use cfdb_core::fact::{Node, PropValue};
use cfdb_core::query::{CompareOp, Expr, NodePattern, Param, Predicate};
use cfdb_core::schema::Label;

use crate::graph::KeyspaceState;
use crate::index::build::IndexValue;
use crate::index::lookup::candidates_from_index;
use crate::index::spec::{ComputedKey, IndexEntry, IndexSpec};

/// Inert bound-var resolver for slice-5 tests: no cross-MATCH
/// hints. Slice-6 tests that exercise cross-ref behaviour build a
/// bespoke closure over a `BTreeMap<(var, prop), IndexValue>`.
fn no_bound(_var: &str, _prop: &str) -> Option<IndexValue> {
    None
}

fn qname_spec() -> IndexSpec {
    IndexSpec {
        entries: vec![
            IndexEntry::Prop {
                label: "Item".into(),
                prop: "qname".into(),
                notes: "test".into(),
            },
            IndexEntry::Prop {
                label: "Item".into(),
                prop: "bounded_context".into(),
                notes: "test".into(),
            },
            IndexEntry::Computed {
                label: "Item".into(),
                computed: ComputedKey::LastSegment,
                notes: "test".into(),
            },
        ],
    }
}

fn state_with_nodes(spec: IndexSpec, nodes: Vec<Node>) -> KeyspaceState {
    let mut state = KeyspaceState::new_with_spec(spec);
    state.ingest_nodes(nodes);
    state
}

fn item(id: &str, qname: &str, ctx: &str) -> Node {
    Node::new(id, Label::new("Item"))
        .with_prop("qname", qname)
        .with_prop("bounded_context", ctx)
}

fn np_item_with_qname(qname: &str) -> NodePattern {
    let mut props = BTreeMap::new();
    props.insert("qname".to_string(), PropValue::from(qname));
    NodePattern {
        var: Some("a".into()),
        label: Some(Label::new("Item")),
        props,
    }
}

fn np_item_var_a() -> NodePattern {
    NodePattern {
        var: Some("a".into()),
        label: Some(Label::new("Item")),
        props: BTreeMap::new(),
    }
}

fn where_a_prop_eq_literal(prop: &str, value: &str) -> Predicate {
    Predicate::Compare {
        left: Expr::Property {
            var: "a".into(),
            prop: prop.into(),
        },
        op: CompareOp::Eq,
        right: Expr::Literal(PropValue::from(value)),
    }
}

#[test]
fn returns_none_when_spec_is_empty() {
    let state = state_with_nodes(IndexSpec::empty(), vec![item("i:1", "foo::a", "ctx")]);
    let np = np_item_with_qname("foo::a");
    assert!(
        candidates_from_index(&state, &np, None, &BTreeMap::new(), &no_bound).is_none(),
        "empty spec must fall through to the label scan"
    );
}

#[test]
fn returns_none_when_label_missing_on_pattern() {
    let state = state_with_nodes(qname_spec(), vec![item("i:1", "foo::a", "ctx")]);
    let mut np = np_item_with_qname("foo::a");
    np.label = None;
    assert!(candidates_from_index(&state, &np, None, &BTreeMap::new(), &no_bound).is_none());
}

#[test]
fn returns_none_when_no_hints_match_spec() {
    let state = state_with_nodes(qname_spec(), vec![item("i:1", "foo::a", "ctx")]);
    // `name` is not an indexed prop.
    let mut np = np_item_var_a();
    np.props.insert("name".into(), PropValue::from("anything"));
    assert!(candidates_from_index(&state, &np, None, &BTreeMap::new(), &no_bound).is_none());
}

#[test]
fn pattern_literal_hits_posting_list() {
    let state = state_with_nodes(
        qname_spec(),
        vec![
            item("i:1", "foo::a", "ctx"),
            item("i:2", "foo::b", "ctx"),
            item("i:3", "foo::a", "other"),
        ],
    );
    let np = np_item_with_qname("foo::a");
    let got = candidates_from_index(&state, &np, None, &BTreeMap::new(), &no_bound)
        .expect("indexed path");
    // i:1 and i:3 both have qname "foo::a".
    assert_eq!(got.len(), 2);
}

#[test]
fn pattern_literal_miss_returns_empty_vec() {
    let state = state_with_nodes(qname_spec(), vec![item("i:1", "foo::a", "ctx")]);
    let np = np_item_with_qname("does::not::exist");
    let got = candidates_from_index(&state, &np, None, &BTreeMap::new(), &no_bound)
        .expect("indexed path");
    assert!(
        got.is_empty(),
        "an indexed miss is still an indexed answer, not a fallback"
    );
}

#[test]
fn where_eq_literal_becomes_a_hint() {
    let state = state_with_nodes(
        qname_spec(),
        vec![item("i:1", "foo::a", "ctx"), item("i:2", "foo::b", "ctx")],
    );
    let np = np_item_var_a();
    let pred = where_a_prop_eq_literal("qname", "foo::a");
    let got = candidates_from_index(&state, &np, Some(&pred), &BTreeMap::new(), &no_bound)
        .expect("indexed path");
    assert_eq!(got.len(), 1);
}

#[test]
fn where_eq_param_becomes_a_hint() {
    let state = state_with_nodes(
        qname_spec(),
        vec![item("i:1", "foo::a", "ctx"), item("i:2", "foo::b", "ctx")],
    );
    let np = np_item_var_a();
    let pred = Predicate::Compare {
        left: Expr::Property {
            var: "a".into(),
            prop: "qname".into(),
        },
        op: CompareOp::Eq,
        right: Expr::Param("q".into()),
    };
    let mut params = BTreeMap::new();
    params.insert("q".to_string(), Param::Scalar(PropValue::from("foo::b")));
    let got =
        candidates_from_index(&state, &np, Some(&pred), &params, &no_bound).expect("indexed path");
    assert_eq!(got.len(), 1);
}

#[test]
fn where_eq_commuted_literal_prop_hits() {
    let state = state_with_nodes(
        qname_spec(),
        vec![item("i:1", "foo::a", "ctx"), item("i:2", "foo::b", "ctx")],
    );
    let np = np_item_var_a();
    let pred = Predicate::Compare {
        left: Expr::Literal(PropValue::from("foo::a")),
        op: CompareOp::Eq,
        right: Expr::Property {
            var: "a".into(),
            prop: "qname".into(),
        },
    };
    let got = candidates_from_index(&state, &np, Some(&pred), &BTreeMap::new(), &no_bound)
        .expect("indexed path");
    assert_eq!(got.len(), 1);
}

#[test]
fn where_and_conjunction_intersects_posting_lists() {
    let state = state_with_nodes(
        qname_spec(),
        vec![
            item("i:1", "foo::a", "ctx1"),
            item("i:2", "foo::a", "ctx2"),
            item("i:3", "foo::b", "ctx1"),
        ],
    );
    let np = np_item_var_a();
    let pred = Predicate::And(
        Box::new(where_a_prop_eq_literal("qname", "foo::a")),
        Box::new(where_a_prop_eq_literal("bounded_context", "ctx1")),
    );
    let got = candidates_from_index(&state, &np, Some(&pred), &BTreeMap::new(), &no_bound)
        .expect("indexed path");
    // Only i:1 matches both.
    assert_eq!(got.len(), 1);
}

#[test]
fn where_or_alone_contributes_no_hint() {
    // `(a.qname = "foo::a") OR (a.qname = "foo::b")` — neither
    // branch is conjunctively joined to the pattern, so neither
    // can seed a posting-list intersection. With no pattern props
    // either, the function returns None and the caller falls back
    // to the label scan (correct: the Evaluator's WHERE filter
    // handles the Or).
    let state = state_with_nodes(
        qname_spec(),
        vec![item("i:1", "foo::a", "ctx"), item("i:2", "foo::b", "ctx")],
    );
    let np = np_item_var_a();
    let pred = Predicate::Or(
        Box::new(where_a_prop_eq_literal("qname", "foo::a")),
        Box::new(where_a_prop_eq_literal("qname", "foo::b")),
    );
    assert!(
        candidates_from_index(&state, &np, Some(&pred), &BTreeMap::new(), &no_bound).is_none(),
        "Or alone contributes no hint; fallback to label scan"
    );
}

#[test]
fn where_not_alone_contributes_no_hint() {
    let state = state_with_nodes(qname_spec(), vec![item("i:1", "foo::a", "ctx")]);
    let np = np_item_var_a();
    let pred = Predicate::Not(Box::new(where_a_prop_eq_literal("qname", "foo::a")));
    assert!(candidates_from_index(&state, &np, Some(&pred), &BTreeMap::new(), &no_bound).is_none());
}

#[test]
fn where_or_inside_and_does_not_invalidate_sibling_hint() {
    // `(a.qname = "foo::a") AND (a.bounded_context = "x"
    //  OR a.bounded_context = "y")` — the qname conjunct is
    // indexable and strictly narrows; the Or sub-tree contributes
    // nothing but must not poison the sibling hint. The outer
    // `run()` WHERE filter will re-evaluate the full predicate
    // (including the Or) on the narrowed candidate set.
    let state = state_with_nodes(
        qname_spec(),
        vec![
            item("i:1", "foo::a", "x"),
            item("i:2", "foo::a", "z"),
            item("i:3", "foo::b", "x"),
        ],
    );
    let np = np_item_var_a();
    let pred = Predicate::And(
        Box::new(where_a_prop_eq_literal("qname", "foo::a")),
        Box::new(Predicate::Or(
            Box::new(where_a_prop_eq_literal("bounded_context", "x")),
            Box::new(where_a_prop_eq_literal("bounded_context", "y")),
        )),
    );
    let got = candidates_from_index(&state, &np, Some(&pred), &BTreeMap::new(), &no_bound)
        .expect("qname hint still valid");
    // i:1 and i:2 share qname foo::a — the Or narrows to i:1
    // post-filter, but candidate_nodes returns both.
    assert_eq!(got.len(), 2);
}

#[test]
fn where_non_eq_compare_is_ignored_not_fatal() {
    let state = state_with_nodes(
        qname_spec(),
        vec![item("i:1", "foo::a", "ctx"), item("i:2", "foo::b", "ctx")],
    );
    let np = np_item_with_qname("foo::a");
    // `a.qname < "zzz"` is not indexable — must not contribute a
    // hint, but must not invalidate the pattern-literal hint that
    // IS indexable.
    let pred = Predicate::Compare {
        left: Expr::Property {
            var: "a".into(),
            prop: "qname".into(),
        },
        op: CompareOp::Lt,
        right: Expr::Literal(PropValue::from("zzz")),
    };
    let got = candidates_from_index(&state, &np, Some(&pred), &BTreeMap::new(), &no_bound)
        .expect("indexed via pattern literal");
    assert_eq!(got.len(), 1);
}

#[test]
fn where_eq_for_unrelated_var_is_ignored() {
    let state = state_with_nodes(
        qname_spec(),
        vec![item("i:1", "foo::a", "ctx"), item("i:2", "foo::b", "ctx")],
    );
    let np = np_item_with_qname("foo::a");
    // `b.qname = "foo::b"` — mentions var `b`, not `a`.
    let pred = Predicate::Compare {
        left: Expr::Property {
            var: "b".into(),
            prop: "qname".into(),
        },
        op: CompareOp::Eq,
        right: Expr::Literal(PropValue::from("foo::b")),
    };
    let got = candidates_from_index(&state, &np, Some(&pred), &BTreeMap::new(), &no_bound)
        .expect("indexed via pattern literal");
    // Pattern literal still narrows to the 1 matching i:1.
    assert_eq!(got.len(), 1);
}


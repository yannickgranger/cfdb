//! Cross-MATCH unit tests for [`crate::index::lookup`]
//! (RFC-035 slice 6 #185).
//!
//! Split out of `lookup_tests.rs` to keep both test files under the
//! workspace 500-LoC god-file ceiling — slice-5 kept its tests in
//! `lookup_tests.rs`; slice-6's cross-MATCH surface needs bespoke
//! helpers (call-expression builder, bound-var resolver over a
//! `BTreeMap`) that would push `lookup_tests.rs` over the limit if
//! co-located.
//!
//! Test shape: construct a small `KeyspaceState` with the slice-6
//! spec (`(Item, qname)` + `(Item, last_segment(qname))` computed),
//! build a `Predicate::Compare` of two `Call(last_segment, ...)`
//! expressions, resolve one side through a stubbed closure over a
//! `BTreeMap<(var, prop), IndexValue>`, and assert the returned
//! `Vec<NodeIndex>` matches the expected posting-list bucket.

use std::collections::BTreeMap;

use cfdb_core::fact::{Node, PropValue};
use cfdb_core::query::{CompareOp, Expr, NodePattern, Predicate};
use cfdb_core::schema::Label;

use crate::graph::KeyspaceState;
use crate::index::build::IndexValue;
use crate::index::lookup::candidates_from_index;
use crate::index::spec::{ComputedKey, IndexEntry, IndexSpec};

fn slice6_spec() -> IndexSpec {
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

fn np_item(var: &str) -> NodePattern {
    NodePattern {
        var: Some(var.into()),
        label: Some(Label::new("Item")),
        props: BTreeMap::new(),
    }
}

/// Build an `Expr::Call { name, args: [Property{var, prop}] }`.
fn call(name: &str, var: &str, prop: &str) -> Expr {
    Expr::Call {
        name: name.into(),
        args: vec![Expr::Property {
            var: var.into(),
            prop: prop.into(),
        }],
    }
}

fn where_computed_eq(
    fn_name: &str,
    left_var: &str,
    left_prop: &str,
    right_var: &str,
    right_prop: &str,
) -> Predicate {
    Predicate::Compare {
        left: call(fn_name, left_var, left_prop),
        op: CompareOp::Eq,
        right: call(fn_name, right_var, right_prop),
    }
}

fn bound_from_map<'a>(
    map: &'a BTreeMap<(&'static str, &'static str), IndexValue>,
) -> impl Fn(&str, &str) -> Option<IndexValue> + 'a {
    move |var, prop| {
        map.iter()
            .find(|((v, p), _)| *v == var && *p == prop)
            .map(|(_, value)| value.clone())
    }
}

#[test]
fn cross_match_resolves_target_b_against_bound_a() {
    // a is bound with qname="some::path::Foo"; target is b; rule:
    // last_segment(a.qname) = last_segment(b.qname). Expect b's
    // candidate set to be the bucket `"Foo"`.
    let state = state_with_nodes(
        slice6_spec(),
        vec![
            item("i:1", "other::path::Foo", "ctx1"),
            item("i:2", "mod::Foo", "ctx2"),
            item("i:3", "mod::Bar", "ctx3"),
        ],
    );
    let np = np_item("b");
    let pred = where_computed_eq("last_segment", "a", "qname", "b", "qname");
    let mut bound_map = BTreeMap::new();
    bound_map.insert(("a", "qname"), "some::path::Foo".to_string());
    let bound = bound_from_map(&bound_map);
    let got = candidates_from_index(&state, &np, Some(&pred), &BTreeMap::new(), &bound)
        .expect("indexed path");
    // i:1 and i:2 both last_segment to "Foo"; i:3 is "Bar".
    assert_eq!(got.len(), 2);
}

#[test]
fn cross_match_resolves_target_a_against_bound_b_commuted() {
    // Commuted version: target is a, b is bound. The hint walker
    // must accept either operand ordering.
    let state = state_with_nodes(
        slice6_spec(),
        vec![
            item("i:1", "x::Foo", "ctx1"),
            item("i:2", "y::Foo", "ctx2"),
            item("i:3", "z::Bar", "ctx3"),
        ],
    );
    let np = np_item("a");
    let pred = where_computed_eq("last_segment", "a", "qname", "b", "qname");
    let mut bound_map = BTreeMap::new();
    bound_map.insert(("b", "qname"), "q::r::Bar".to_string());
    let bound = bound_from_map(&bound_map);
    let got = candidates_from_index(&state, &np, Some(&pred), &BTreeMap::new(), &bound)
        .expect("indexed path");
    // i:3 is the only Bar.
    assert_eq!(got.len(), 1);
}

#[test]
fn cross_match_falls_through_when_bound_var_unresolved() {
    // Bound side references var "a", but the resolver returns None
    // (simulates unbound from a previous pattern). The hint is
    // skipped; no other hint applies; function returns None.
    let state = state_with_nodes(slice6_spec(), vec![item("i:1", "x::Foo", "ctx")]);
    let np = np_item("b");
    let pred = where_computed_eq("last_segment", "a", "qname", "b", "qname");
    let empty_map: BTreeMap<(&'static str, &'static str), IndexValue> = BTreeMap::new();
    let bound = bound_from_map(&empty_map);
    assert!(
        candidates_from_index(&state, &np, Some(&pred), &BTreeMap::new(), &bound).is_none(),
        "unresolved bound var must skip the cross-ref hint; no other hint → None"
    );
}

#[test]
fn cross_match_skips_unknown_call_name() {
    // Unrecognised fn: no hint is emitted. With no pattern hints
    // either, the function returns None.
    let state = state_with_nodes(slice6_spec(), vec![item("i:1", "x::Foo", "ctx")]);
    let np = np_item("b");
    let pred = where_computed_eq("not_a_computed_key", "a", "qname", "b", "qname");
    let mut bound_map = BTreeMap::new();
    bound_map.insert(("a", "qname"), "x::Foo".to_string());
    let bound = bound_from_map(&bound_map);
    assert!(candidates_from_index(&state, &np, Some(&pred), &BTreeMap::new(), &bound).is_none());
}

#[test]
fn cross_match_skips_when_spec_lacks_computed_entry() {
    // Spec has `(Item, qname)` prop index but NOT the `(Item,
    // last_segment(qname))` computed index. The cross-ref hint is
    // legal-shape but no posting list exists — fallback.
    let spec_without_computed = IndexSpec {
        entries: vec![IndexEntry::Prop {
            label: "Item".into(),
            prop: "qname".into(),
            notes: "test".into(),
        }],
    };
    let state = state_with_nodes(spec_without_computed, vec![item("i:1", "x::Foo", "ctx")]);
    let np = np_item("b");
    let pred = where_computed_eq("last_segment", "a", "qname", "b", "qname");
    let mut bound_map = BTreeMap::new();
    bound_map.insert(("a", "qname"), "x::Foo".to_string());
    let bound = bound_from_map(&bound_map);
    assert!(
        candidates_from_index(&state, &np, Some(&pred), &BTreeMap::new(), &bound).is_none(),
        "computed-key hint must be rejected when IndexEntry::Computed isn't in the spec"
    );
}

#[test]
fn cross_match_skips_when_both_sides_are_target_var() {
    // `last_segment(b.qname) = last_segment(b.name)` — same var on
    // both sides. This is not cross-MATCH; it's a single-variable
    // constraint (and currently unsupported by the fast path). No
    // hint emitted.
    let state = state_with_nodes(slice6_spec(), vec![item("i:1", "x::Foo", "ctx")]);
    let np = np_item("b");
    let pred = where_computed_eq("last_segment", "b", "qname", "b", "name");
    let mut bound_map = BTreeMap::new();
    bound_map.insert(("b", "qname"), "x::Foo".to_string());
    let bound = bound_from_map(&bound_map);
    assert!(candidates_from_index(&state, &np, Some(&pred), &BTreeMap::new(), &bound).is_none());
}

#[test]
fn cross_match_skips_when_neither_side_is_target_var() {
    // `last_segment(a.qname) = last_segment(c.qname)` — target is
    // `b`, neither side mentions it. No hint.
    let state = state_with_nodes(slice6_spec(), vec![item("i:1", "x::Foo", "ctx")]);
    let np = np_item("b");
    let pred = where_computed_eq("last_segment", "a", "qname", "c", "qname");
    let mut bound_map = BTreeMap::new();
    bound_map.insert(("a", "qname"), "x::Foo".to_string());
    bound_map.insert(("c", "qname"), "y::Foo".to_string());
    let bound = bound_from_map(&bound_map);
    assert!(candidates_from_index(&state, &np, Some(&pred), &BTreeMap::new(), &bound).is_none());
}

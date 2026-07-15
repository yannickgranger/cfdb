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
use crate::index::spec::{ComputedKey, IndexEntry, IndexSpec, CONVERSION_PREFIX_PATTERN};

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

// --- Slice 6b: plain Property = Property cross-MATCH hint --------

fn name_indexed_spec() -> IndexSpec {
    IndexSpec {
        entries: vec![
            IndexEntry::Prop {
                label: "Item".into(),
                prop: "name".into(),
                notes: "test — slice 6b prop-to-prop equi-join".into(),
            },
            IndexEntry::Prop {
                label: "Item".into(),
                prop: "bounded_context".into(),
                notes: "test".into(),
            },
        ],
    }
}

fn item_named(id: &str, name: &str, ctx: &str) -> Node {
    Node::new(id, Label::new("Item"))
        .with_prop("name", name)
        .with_prop("bounded_context", ctx)
}

fn where_prop_eq(left_var: &str, left_prop: &str, right_var: &str, right_prop: &str) -> Predicate {
    Predicate::Compare {
        left: Expr::Property {
            var: left_var.into(),
            prop: left_prop.into(),
        },
        op: CompareOp::Eq,
        right: Expr::Property {
            var: right_var.into(),
            prop: right_prop.into(),
        },
    }
}

#[test]
fn cross_match_prop_eq_resolves_target_b_against_bound_a() {
    let state = state_with_nodes(
        name_indexed_spec(),
        vec![
            item_named("i:1", "Foo", "ctx_a"),
            item_named("i:2", "Bar", "ctx_a"),
            item_named("i:3", "Foo", "ctx_b"),
            item_named("i:4", "Baz", "ctx_c"),
        ],
    );
    let np = np_item("b");
    let pred = where_prop_eq("a", "name", "b", "name");
    let mut bound_map = BTreeMap::new();
    bound_map.insert(("a", "name"), "Foo".to_string());
    let bound = bound_from_map(&bound_map);
    let got = candidates_from_index(&state, &np, Some(&pred), &BTreeMap::new(), &bound)
        .expect("indexed prop-to-prop path");
    assert_eq!(got.len(), 2, "i:1 and i:3 both have name=Foo");
}

#[test]
fn cross_match_prop_eq_resolves_target_a_against_bound_b_commuted() {
    let state = state_with_nodes(
        name_indexed_spec(),
        vec![
            item_named("i:1", "Foo", "ctx_a"),
            item_named("i:2", "Bar", "ctx_a"),
            item_named("i:3", "Foo", "ctx_b"),
        ],
    );
    let np = np_item("a");
    let pred = where_prop_eq("a", "name", "b", "name");
    let mut bound_map = BTreeMap::new();
    bound_map.insert(("b", "name"), "Bar".to_string());
    let bound = bound_from_map(&bound_map);
    let got = candidates_from_index(&state, &np, Some(&pred), &BTreeMap::new(), &bound)
        .expect("indexed prop-to-prop path");
    assert_eq!(got.len(), 1, "only i:2 has name=Bar");
}

#[test]
fn cross_match_prop_eq_falls_through_when_bound_var_unresolved() {
    let state = state_with_nodes(name_indexed_spec(), vec![item_named("i:1", "Foo", "ctx")]);
    let np = np_item("b");
    let pred = where_prop_eq("a", "name", "b", "name");
    let empty_map: BTreeMap<(&'static str, &'static str), IndexValue> = BTreeMap::new();
    let bound = bound_from_map(&empty_map);
    assert!(
        candidates_from_index(&state, &np, Some(&pred), &BTreeMap::new(), &bound).is_none(),
        "unresolved bound var must skip the cross-ref hint; no other hint → None"
    );
}

#[test]
fn cross_match_prop_eq_skips_when_prop_not_indexed() {
    let spec_no_name = IndexSpec {
        entries: vec![IndexEntry::Prop {
            label: "Item".into(),
            prop: "bounded_context".into(),
            notes: "test".into(),
        }],
    };
    let state = state_with_nodes(spec_no_name, vec![item_named("i:1", "Foo", "ctx")]);
    let np = np_item("b");
    let pred = where_prop_eq("a", "name", "b", "name");
    let mut bound_map = BTreeMap::new();
    bound_map.insert(("a", "name"), "Foo".to_string());
    let bound = bound_from_map(&bound_map);
    assert!(
        candidates_from_index(&state, &np, Some(&pred), &BTreeMap::new(), &bound).is_none(),
        "prop-to-prop hint must be rejected when (label, prop) isn't indexed"
    );
}

#[test]
fn cross_match_prop_eq_skips_when_props_differ() {
    // `a.name = b.crate` — different props on the two sides cannot
    // hash on one posting list even if both are individually indexed.
    let spec_both = IndexSpec {
        entries: vec![
            IndexEntry::Prop {
                label: "Item".into(),
                prop: "name".into(),
                notes: "test".into(),
            },
            IndexEntry::Prop {
                label: "Item".into(),
                prop: "crate".into(),
                notes: "test".into(),
            },
        ],
    };
    let state = state_with_nodes(spec_both, vec![item_named("i:1", "Foo", "ctx")]);
    let np = np_item("b");
    let pred = where_prop_eq("a", "name", "b", "crate");
    let mut bound_map = BTreeMap::new();
    bound_map.insert(("a", "name"), "Foo".to_string());
    let bound = bound_from_map(&bound_map);
    assert!(
        candidates_from_index(&state, &np, Some(&pred), &BTreeMap::new(), &bound).is_none(),
        "different-prop equi-join must not produce a single-bucket hint"
    );
}

#[test]
fn cross_match_prop_eq_skips_when_both_sides_are_target_var() {
    let state = state_with_nodes(name_indexed_spec(), vec![item_named("i:1", "Foo", "ctx")]);
    let np = np_item("b");
    let pred = where_prop_eq("b", "name", "b", "name");
    let mut bound_map = BTreeMap::new();
    bound_map.insert(("b", "name"), "Foo".to_string());
    let bound = bound_from_map(&bound_map);
    assert!(candidates_from_index(&state, &np, Some(&pred), &BTreeMap::new(), &bound).is_none());
}

#[test]
fn cross_match_prop_eq_skips_when_neither_side_is_target_var() {
    let state = state_with_nodes(name_indexed_spec(), vec![item_named("i:1", "Foo", "ctx")]);
    let np = np_item("b");
    let pred = where_prop_eq("a", "name", "c", "name");
    let mut bound_map = BTreeMap::new();
    bound_map.insert(("a", "name"), "Foo".to_string());
    bound_map.insert(("c", "name"), "Foo".to_string());
    let bound = bound_from_map(&bound_map);
    assert!(candidates_from_index(&state, &np, Some(&pred), &BTreeMap::new(), &bound).is_none());
}

// --- Slice 6: ConversionPrefix computed-key cross-MATCH ----------
//
// `regexp_extract(a.name, '<vetted>') = regexp_extract(b.name,
// '<vetted>')` — the RandomScattering fork join. Recognition is
// byte-for-byte on the pattern literal; a bound name with no conversion
// prefix is a NULL join operand that narrows the target to empty.

fn conversion_prefix_spec() -> IndexSpec {
    IndexSpec {
        entries: vec![IndexEntry::Computed {
            label: "Item".into(),
            computed: ComputedKey::ConversionPrefix,
            notes: "test — RandomScattering fork bucket".into(),
        }],
    }
}

/// `Expr::Call { name: "regexp_extract", args: [Property{var, prop},
/// Literal(Str(pattern))] }`.
fn call_regexp(var: &str, prop: &str, pattern: &str) -> Expr {
    Expr::Call {
        name: "regexp_extract".into(),
        args: vec![
            Expr::Property {
                var: var.into(),
                prop: prop.into(),
            },
            Expr::Literal(PropValue::Str(pattern.into())),
        ],
    }
}

fn where_regexp_eq(
    left_var: &str,
    left_prop: &str,
    right_var: &str,
    right_prop: &str,
    pattern: &str,
) -> Predicate {
    Predicate::Compare {
        left: call_regexp(left_var, left_prop, pattern),
        op: CompareOp::Eq,
        right: call_regexp(right_var, right_prop, pattern),
    }
}

#[test]
fn cross_match_conversion_prefix_resolves_target_b_against_bound_a() {
    // a bound with name "compute_0_from_bps" → prefix "compute_0_from_".
    // b's candidates are the two items in that bucket; the third item
    // has a different prefix and the fourth (no conversion prefix) has
    // no posting at all.
    let state = state_with_nodes(
        conversion_prefix_spec(),
        vec![
            item_named("i:1", "compute_0_from_bps", "ctx"),
            item_named("i:2", "compute_0_from_pct", "ctx"),
            item_named("i:3", "compute_1_from_bps", "ctx"),
            item_named("i:4", "uniq_9", "ctx"),
        ],
    );
    let np = np_item("b");
    let pred = where_regexp_eq("a", "name", "b", "name", CONVERSION_PREFIX_PATTERN);
    let mut bound_map = BTreeMap::new();
    bound_map.insert(("a", "name"), "compute_0_from_bps".to_string());
    let bound = bound_from_map(&bound_map);
    let got = candidates_from_index(&state, &np, Some(&pred), &BTreeMap::new(), &bound)
        .expect("indexed conversion-prefix path");
    assert_eq!(got.len(), 2, "i:1 and i:2 share prefix compute_0_from_");
}

#[test]
fn cross_match_conversion_prefix_resolves_target_a_against_bound_b_commuted() {
    let state = state_with_nodes(
        conversion_prefix_spec(),
        vec![
            item_named("i:1", "qty_to_notional", "ctx"),
            item_named("i:2", "qty_to_base", "ctx"),
            item_named("i:3", "price_for_side", "ctx"),
        ],
    );
    let np = np_item("a");
    let pred = where_regexp_eq("a", "name", "b", "name", CONVERSION_PREFIX_PATTERN);
    let mut bound_map = BTreeMap::new();
    bound_map.insert(("b", "name"), "qty_to_notional".to_string());
    let bound = bound_from_map(&bound_map);
    let got = candidates_from_index(&state, &np, Some(&pred), &BTreeMap::new(), &bound)
        .expect("indexed conversion-prefix path");
    assert_eq!(got.len(), 2, "i:1 and i:2 share prefix qty_to_");
}

#[test]
fn cross_match_conversion_prefix_non_matching_bound_narrows_to_empty() {
    // Bound name has no conversion prefix → evaluate is None → the
    // equi-join is unsatisfiable (NULL = anything is false). The fast
    // path narrows the target to the EMPTY set rather than falling back
    // to a full scan — Some(empty), not None.
    let state = state_with_nodes(
        conversion_prefix_spec(),
        vec![
            item_named("i:1", "compute_0_from_bps", "ctx"),
            item_named("i:2", "compute_0_from_pct", "ctx"),
        ],
    );
    let np = np_item("b");
    let pred = where_regexp_eq("a", "name", "b", "name", CONVERSION_PREFIX_PATTERN);
    let mut bound_map = BTreeMap::new();
    bound_map.insert(("a", "name"), "uniq_9".to_string());
    let bound = bound_from_map(&bound_map);
    let got = candidates_from_index(&state, &np, Some(&pred), &BTreeMap::new(), &bound)
        .expect("provably-empty commits to an empty answer, not a fallback");
    assert!(
        got.is_empty(),
        "a NULL join operand narrows the target to empty; got {got:?}"
    );
}

#[test]
fn cross_match_conversion_prefix_empty_narrow_gated_on_index_present() {
    // Same non-matching bound, but the conversion_prefix computed index
    // is NOT in the spec. The empty-narrow is gated on the key being
    // indexed (like every hint), so the fast path declines and the
    // caller falls back to the full scan — None, not Some(empty).
    let spec_without_computed = IndexSpec {
        entries: vec![IndexEntry::Prop {
            label: "Item".into(),
            prop: "name".into(),
            notes: "test".into(),
        }],
    };
    let state = state_with_nodes(
        spec_without_computed,
        vec![item_named("i:1", "compute_0_from_bps", "ctx")],
    );
    let np = np_item("b");
    let pred = where_regexp_eq("a", "name", "b", "name", CONVERSION_PREFIX_PATTERN);
    let mut bound_map = BTreeMap::new();
    bound_map.insert(("a", "name"), "uniq_9".to_string());
    let bound = bound_from_map(&bound_map);
    assert!(
        candidates_from_index(&state, &np, Some(&pred), &BTreeMap::new(), &bound).is_none(),
        "empty-narrow must be gated on the computed key being indexed"
    );
}

#[test]
fn cross_match_conversion_prefix_wrong_literal_no_hint() {
    // A `regexp_extract` whose pattern literal is NOT the vetted const
    // is not the conversion-prefix join — no hint, fall back. Guards the
    // byte-for-byte recognition contract.
    let state = state_with_nodes(
        conversion_prefix_spec(),
        vec![item_named("i:1", "compute_0_from_bps", "ctx")],
    );
    let np = np_item("b");
    let pred = where_regexp_eq("a", "name", "b", "name", r"^(\w+)_(?:via)_");
    let mut bound_map = BTreeMap::new();
    bound_map.insert(("a", "name"), "compute_0_from_bps".to_string());
    let bound = bound_from_map(&bound_map);
    assert!(
        candidates_from_index(&state, &np, Some(&pred), &BTreeMap::new(), &bound).is_none(),
        "only the vetted pattern literal is recognised as conversion_prefix"
    );
}

#[test]
fn cross_match_conversion_prefix_skips_when_call_reads_wrong_prop() {
    // `regexp_extract(a.qname, '<vetted>')` reads `qname`, but
    // ConversionPrefix's source prop is `name`; the posting list is
    // keyed off `name`, so a qname-sourced call must NOT narrow through
    // it. No hint.
    let state = state_with_nodes(
        conversion_prefix_spec(),
        vec![item_named("i:1", "compute_0_from_bps", "ctx")],
    );
    let np = np_item("b");
    let pred = where_regexp_eq("a", "qname", "b", "qname", CONVERSION_PREFIX_PATTERN);
    let mut bound_map = BTreeMap::new();
    bound_map.insert(("a", "qname"), "crate::compute_0_from_bps".to_string());
    let bound = bound_from_map(&bound_map);
    assert!(
        candidates_from_index(&state, &np, Some(&pred), &BTreeMap::new(), &bound).is_none(),
        "source-prop mismatch (qname vs name) must not produce a hint"
    );
}

#[test]
fn cross_match_conversion_prefix_falls_through_when_bound_var_unresolved() {
    // Bound side unresolved (e.g. the outer `a` MATCH, before `b` is
    // bound) — no hint, no empty-narrow. This is what keeps the outer
    // scan from wrongly collapsing to empty.
    let state = state_with_nodes(
        conversion_prefix_spec(),
        vec![item_named("i:1", "compute_0_from_bps", "ctx")],
    );
    let np = np_item("b");
    let pred = where_regexp_eq("a", "name", "b", "name", CONVERSION_PREFIX_PATTERN);
    let empty_map: BTreeMap<(&'static str, &'static str), IndexValue> = BTreeMap::new();
    let bound = bound_from_map(&empty_map);
    assert!(
        candidates_from_index(&state, &np, Some(&pred), &BTreeMap::new(), &bound).is_none(),
        "unresolved bound var must skip the conversion-prefix hint entirely"
    );
}

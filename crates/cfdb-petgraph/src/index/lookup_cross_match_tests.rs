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
    assert_eq!(got.len(), 2);
}

#[test]
fn cross_match_resolves_target_a_against_bound_b_commuted() {
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
    assert_eq!(got.len(), 1);
}

#[test]
fn cross_match_falls_through_when_bound_var_unresolved() {
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
    let state = state_with_nodes(slice6_spec(), vec![item("i:1", "x::Foo", "ctx")]);
    let np = np_item("b");
    let pred = where_computed_eq("last_segment", "a", "qname", "c", "qname");
    let mut bound_map = BTreeMap::new();
    bound_map.insert(("a", "qname"), "x::Foo".to_string());
    bound_map.insert(("c", "qname"), "y::Foo".to_string());
    let bound = bound_from_map(&bound_map);
    assert!(candidates_from_index(&state, &np, Some(&pred), &BTreeMap::new(), &bound).is_none());
}

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

fn conversion_prefix_spec() -> IndexSpec {
    IndexSpec {
        entries: vec![IndexEntry::Computed {
            label: "Item".into(),
            computed: ComputedKey::ConversionPrefix,
            notes: "test — RandomScattering fork bucket".into(),
        }],
    }
}

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

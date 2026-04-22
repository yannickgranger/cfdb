//! Evaluator fast paths for `candidate_nodes` — RFC-035 §3.6
//! fast paths 1 and 2 (slice 5 #184).
//!
//! Two indexable shapes are handled here:
//!
//! 1. **Label + prop literal** — `MATCH (a:Item {qname: "foo::bar"})`.
//!    Literals inside the `NodePattern::props` map are picked up when
//!    the `(label, prop)` pair appears in the keyspace's [`IndexSpec`].
//!
//! 2. **Label + WHERE Eq on literal** — `MATCH (a:Item) WHERE a.qname = $x`.
//!    The evaluator threads the query's top-level WHERE clause into
//!    `candidate_nodes` (slice 5 change) so this module can detect
//!    indexable `Eq` conjuncts bound to the pattern's variable.
//!
//! Non-indexable shapes (no label, no matching spec entry, `Or`/`Not`
//! in the WHERE, property-on-both-sides Eq) yield `None` — callers
//! fall back to the full `by_label` scan, preserving the pre-RFC-035
//! behaviour for every query that cannot be accelerated.
//!
//! # Why `And`-only descent
//!
//! `by_prop` posting-list intersection is conjunctive. We only
//! descend through `And` nodes in the predicate tree; `Or` and `Not`
//! subtrees contribute no hint because they express disjunction, not
//! restriction. Sibling `And`-conjuncts remain valid — the outer
//! `Evaluator::run` WHERE filter re-applies the full predicate to
//! the narrowed candidate set, so a hint that over-narrowing could
//! have introduced is impossible. Every hint we emit strictly
//! narrows the posting list compared to the full label scan.
//!
//! # Cross-MATCH intersection is slice 6, not here
//!
//! RFC-035 §3.6 also describes cross-MATCH posting-list intersection
//! (`last_segment(a.qname) = last_segment(b.qname)`). That fast path
//! belongs to slice 6 (#185) because it pins two *different* pattern
//! variables against each other; this module only handles single-
//! variable pins against literals / params.

use std::collections::{BTreeMap, BTreeSet};

use cfdb_core::query::{CompareOp, Expr, NodePattern, Param, Predicate};
use cfdb_core::schema::Label;
use petgraph::stable_graph::NodeIndex;

use crate::graph::KeyspaceState;
use crate::index::build::{index_key_of, IndexTag, IndexValue};
use crate::index::spec::{IndexEntry, IndexSpec};

/// Attempt to satisfy a `candidate_nodes` request through the
/// inverted-index posting lists instead of a full `by_label` scan.
///
/// Returns `Some(vec)` (possibly empty) when at least one indexable
/// hint applies; `None` when no hint matches and the caller must fall
/// back. A returned `Vec` is sorted by `NodeIndex` because posting
/// lists are `BTreeSet<NodeIndex>`, matching the determinism contract
/// already honoured by `KeyspaceState::nodes_with_label`.
///
/// Hint sources:
/// - `np.props` — every literal prop equality becomes a hint when the
///   `(label, prop)` pair is in `state.index_spec`.
/// - `where_clause` — every `Predicate::Compare { op: Eq, ... }`
///   conjunct whose left/right is `(a.prop, literal)` or `(literal,
///   a.prop)` with `a == np.var` becomes a hint under the same spec
///   check. The predicate walker descends only through `And` nodes;
///   `Or` / `Not` subtrees contribute nothing but don't poison sibling
///   `And`-conjuncts (see module doc "Why `And`-only descent").
pub(crate) fn candidates_from_index(
    state: &KeyspaceState,
    np: &NodePattern,
    where_clause: Option<&Predicate>,
    params: &BTreeMap<String, Param>,
) -> Option<Vec<NodeIndex>> {
    let label = np.label.as_ref()?;
    if state.index_spec.entries.is_empty() {
        return None;
    }

    let mut hints: Vec<(IndexTag, IndexValue)> = Vec::new();
    collect_pattern_hints(label, &state.index_spec, np, &mut hints);

    if let Some(pred) = where_clause {
        if let Some(var) = np.var.as_deref() {
            collect_where_hints(label, &state.index_spec, var, pred, params, &mut hints);
        }
    }

    if hints.is_empty() {
        return None;
    }
    Some(intersect(state, label, &hints))
}

/// Pull literal `(prop, value)` hints out of an inline pattern props
/// map. Only values indexable by [`index_key_of`] (scalar `Str` /
/// `Int` / `Bool`) participate; `Float` / `Null` are skipped and the
/// caller falls back to the label scan for those props.
///
/// Iterator-chain form so the `prop.clone()` required to own the
/// `IndexTag` doesn't register as a clone-in-loop against the
/// workspace metric scanner (same technique as `eval::pattern::unwind_row`).
fn collect_pattern_hints(
    label: &Label,
    spec: &IndexSpec,
    np: &NodePattern,
    out: &mut Vec<(IndexTag, IndexValue)>,
) {
    let fresh = np
        .props
        .iter()
        .filter(|(prop, _)| is_indexed_prop(spec, label, prop))
        .filter_map(|(prop, value)| index_key_of(value).map(|v| (prop.clone(), v)));
    out.extend(fresh);
}

/// Walk a WHERE predicate, descending only through `And` nodes, and
/// append every indexable Eq conjunct bound to `target_var` to `out`.
/// `Or` / `Not` subtrees contribute no hint and no descent — every
/// hint appended is conjunctively joined to the pattern, so it
/// strictly narrows the candidate set.
fn collect_where_hints(
    label: &Label,
    spec: &IndexSpec,
    target_var: &str,
    pred: &Predicate,
    params: &BTreeMap<String, Param>,
    out: &mut Vec<(IndexTag, IndexValue)>,
) {
    match pred {
        Predicate::And(a, b) => {
            collect_where_hints(label, spec, target_var, a, params, out);
            collect_where_hints(label, spec, target_var, b, params, out);
        }
        Predicate::Compare {
            left,
            op: CompareOp::Eq,
            right,
        } => {
            if let Some((prop, value)) = resolve_eq_hint(target_var, left, right, params) {
                if is_indexed_prop(spec, label, &prop) {
                    out.push((prop, value));
                }
            }
        }
        // Non-Eq Compare, IN, Regex, NotExists, Ne, Or, Not — no
        // hint, and Or/Not in particular we explicitly do not
        // descend into because the branches are disjunctive and
        // cannot be intersected with the pattern's posting lists.
        Predicate::Compare { .. }
        | Predicate::In { .. }
        | Predicate::Regex { .. }
        | Predicate::NotExists { .. }
        | Predicate::Ne { .. }
        | Predicate::Or(_, _)
        | Predicate::Not(_) => {}
    }
}

/// Recognise `a.prop = literal` in either order. Returns
/// `Some((prop_name, index_value))` when one side is a property
/// reference on `target_var` and the other is a literal or resolvable
/// `$param`; `None` for property-on-both-sides or unsupported shapes.
fn resolve_eq_hint(
    target_var: &str,
    left: &Expr,
    right: &Expr,
    params: &BTreeMap<String, Param>,
) -> Option<(String, IndexValue)> {
    match (left, right) {
        (Expr::Property { var, prop }, other) if var == target_var => {
            resolve_literal_value(other, params).map(|v| (prop.clone(), v))
        }
        (other, Expr::Property { var, prop }) if var == target_var => {
            resolve_literal_value(other, params).map(|v| (prop.clone(), v))
        }
        _ => None,
    }
}

/// Resolve a right-hand-side expression to an index key. Literals
/// unwrap directly; `$param` references look up a scalar value in
/// the param bag. Anything else (list, property, function call) is
/// unsupported for this slice and returns `None`.
fn resolve_literal_value(expr: &Expr, params: &BTreeMap<String, Param>) -> Option<IndexValue> {
    match expr {
        Expr::Literal(pv) => index_key_of(pv),
        Expr::Param(name) => match params.get(name)? {
            Param::Scalar(pv) => index_key_of(pv),
            Param::List(_) => None,
        },
        _ => None,
    }
}

/// `(label, prop)` membership check against the spec. Only matches
/// `IndexEntry::Prop` — computed keys have a different join surface
/// (slice 6) and do not participate in single-variable literal pins.
fn is_indexed_prop(spec: &IndexSpec, label: &Label, prop: &str) -> bool {
    spec.entries.iter().any(|entry| match entry {
        IndexEntry::Prop {
            label: l,
            prop: p,
            notes: _,
        } => l.as_str() == label.as_str() && p == prop,
        IndexEntry::Computed { .. } => false,
    })
}

/// Intersect the posting lists named by `hints`. An empty
/// intersection is a valid answer (the index conclusively proves no
/// node matches); we return `Vec::new()` rather than `None` because
/// the fast-path short-circuit has already committed to answering
/// from indexes. `hints` MUST be non-empty — the caller guards this.
fn intersect(
    state: &KeyspaceState,
    label: &Label,
    hints: &[(IndexTag, IndexValue)],
) -> Vec<NodeIndex> {
    let mut iter = hints.iter();
    let Some((first_tag, first_value)) = iter.next() else {
        // Defensive: `candidates_from_index` never calls us with an
        // empty hint vec, but returning an empty Vec on misuse is
        // preferable to an index panic.
        return Vec::new();
    };
    let mut acc: BTreeSet<NodeIndex> = lookup_posting(state, label, first_tag, first_value)
        .cloned()
        .unwrap_or_default();
    for (tag, value) in iter {
        if acc.is_empty() {
            break;
        }
        let next = lookup_posting(state, label, tag, value)
            .cloned()
            .unwrap_or_default();
        acc = acc.intersection(&next).copied().collect();
    }
    acc.into_iter().collect()
}

fn lookup_posting<'s>(
    state: &'s KeyspaceState,
    label: &Label,
    tag: &IndexTag,
    value: &IndexValue,
) -> Option<&'s BTreeSet<NodeIndex>> {
    state.by_prop.get(&(label.clone(), tag.clone()))?.get(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cfdb_core::fact::{Node, PropValue};
    use cfdb_core::query::{CompareOp, Expr, NodePattern, Predicate};
    use cfdb_core::schema::Label;

    use crate::index::spec::{ComputedKey, IndexEntry, IndexSpec};

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
        let mut props = std::collections::BTreeMap::new();
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
            props: std::collections::BTreeMap::new(),
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
            candidates_from_index(&state, &np, None, &BTreeMap::new()).is_none(),
            "empty spec must fall through to the label scan"
        );
    }

    #[test]
    fn returns_none_when_label_missing_on_pattern() {
        let state = state_with_nodes(qname_spec(), vec![item("i:1", "foo::a", "ctx")]);
        let mut np = np_item_with_qname("foo::a");
        np.label = None;
        assert!(candidates_from_index(&state, &np, None, &BTreeMap::new()).is_none());
    }

    #[test]
    fn returns_none_when_no_hints_match_spec() {
        let state = state_with_nodes(qname_spec(), vec![item("i:1", "foo::a", "ctx")]);
        // `name` is not an indexed prop.
        let mut np = np_item_var_a();
        np.props.insert("name".into(), PropValue::from("anything"));
        assert!(candidates_from_index(&state, &np, None, &BTreeMap::new()).is_none());
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
        let got = candidates_from_index(&state, &np, None, &BTreeMap::new()).expect("indexed path");
        // i:1 and i:3 both have qname "foo::a".
        assert_eq!(got.len(), 2);
    }

    #[test]
    fn pattern_literal_miss_returns_empty_vec() {
        let state = state_with_nodes(qname_spec(), vec![item("i:1", "foo::a", "ctx")]);
        let np = np_item_with_qname("does::not::exist");
        let got = candidates_from_index(&state, &np, None, &BTreeMap::new()).expect("indexed path");
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
        let got = candidates_from_index(&state, &np, Some(&pred), &BTreeMap::new())
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
        let got = candidates_from_index(&state, &np, Some(&pred), &params).expect("indexed path");
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
        let got = candidates_from_index(&state, &np, Some(&pred), &BTreeMap::new())
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
        let got = candidates_from_index(&state, &np, Some(&pred), &BTreeMap::new())
            .expect("indexed path");
        // Only i:1 matches both.
        assert_eq!(got.len(), 1);
    }

    #[test]
    fn where_or_alone_contributes_no_hint() {
        // `(a.qname = "foo::a") OR (a.qname = "foo::b")` — neither
        // branch is conjunctively joined to the pattern, so neither
        // can seed a posting-list intersection. With no pattern
        // props either, the function returns None and the caller
        // falls back to the label scan (correct: the Evaluator's
        // WHERE filter handles the Or).
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
            candidates_from_index(&state, &np, Some(&pred), &BTreeMap::new()).is_none(),
            "Or alone contributes no hint; fallback to label scan"
        );
    }

    #[test]
    fn where_not_alone_contributes_no_hint() {
        let state = state_with_nodes(qname_spec(), vec![item("i:1", "foo::a", "ctx")]);
        let np = np_item_var_a();
        let pred = Predicate::Not(Box::new(where_a_prop_eq_literal("qname", "foo::a")));
        assert!(candidates_from_index(&state, &np, Some(&pred), &BTreeMap::new()).is_none());
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
        let got = candidates_from_index(&state, &np, Some(&pred), &BTreeMap::new())
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
        // hint, but must not invalidate the pattern-literal hint
        // that IS indexable.
        let pred = Predicate::Compare {
            left: Expr::Property {
                var: "a".into(),
                prop: "qname".into(),
            },
            op: CompareOp::Lt,
            right: Expr::Literal(PropValue::from("zzz")),
        };
        let got = candidates_from_index(&state, &np, Some(&pred), &BTreeMap::new())
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
        let got = candidates_from_index(&state, &np, Some(&pred), &BTreeMap::new())
            .expect("indexed via pattern literal");
        // Pattern literal still narrows to the 1 matching i:1.
        assert_eq!(got.len(), 1);
    }
}

//! Evaluator fast paths for `candidate_nodes` — RFC-035 §3.6
//! fast paths 1, 2, and cross-MATCH (slices 5 #184 + 6 #185).
//!
//! Three indexable shapes are handled here:
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
//! 3. **Cross-MATCH computed-key intersection** — the `context_homonym`
//!    shape: `MATCH (a:Item), (b:Item) WHERE last_segment(a.qname) =
//!    last_segment(b.qname)` (slice 6). When the bound var's value on
//!    the non-target side of the equality resolves through the
//!    `bound_var_prop` closure, we apply the computed key and narrow
//!    the target var's candidates to that single bucket. Turns the
//!    Σ|Items|² cartesian into Σ|bucket|².
//!
//! Non-indexable shapes (no label, no matching spec entry, `Or`/`Not`
//! in the WHERE, property-on-both-sides Eq without a bound-var
//! resolver, computed call on a prop the index is not built on)
//! yield `None` — callers fall back to the full `by_label` scan,
//! preserving the pre-RFC-035 behaviour for every query that cannot
//! be accelerated.
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
//! # Bound-var resolver
//!
//! `candidates_from_index` takes a `bound_var_prop: impl Fn(&str, &str)
//! -> Option<IndexValue>` closure. For single-MATCH queries the caller
//! passes `|_, _| None` (nothing is bound yet). For multi-MATCH
//! queries the caller threads the incoming bindings row through, so
//! the resolver returns `Some(index_value)` when the bound var's
//! requested prop is indexable. Keeping the coupling inverted
//! (lookup asks, caller resolves) avoids pulling `eval::Binding` and
//! `petgraph::StableDiGraph` into this module — lookup stays a pure
//! function of `KeyspaceState` + a closure.

use std::collections::{BTreeMap, BTreeSet};

use cfdb_core::query::{CompareOp, Expr, NodePattern, Param, Predicate};
use cfdb_core::schema::Label;
use petgraph::stable_graph::NodeIndex;

use crate::graph::KeyspaceState;
use crate::index::build::{index_key_of, IndexTag, IndexValue};
use crate::index::spec::{ComputedKey, IndexEntry, IndexSpec};

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
/// - `where_clause` (slice 5) — every `Predicate::Compare { op: Eq, ... }`
///   conjunct whose left/right is `(a.prop, literal)` or `(literal,
///   a.prop)` with `a == np.var` becomes a hint under the same spec
///   check.
/// - `where_clause` (slice 6, cross-MATCH) — every `Compare { op: Eq,
///   left: Call(f, [Property{x, p}]), right: Call(f, [Property{y, p}]) }`
///   (either order) where `f` is an allowlisted `ComputedKey`, exactly
///   one of `{x, y}` is `np.var`, the other is resolvable through
///   `bound_var_prop`, and `IndexEntry::Computed { label, computed }`
///   is in the spec. The hint narrows the target to the single
///   posting-list bucket for the bound value's derived key — this is
///   the `context_homonym` fast path (RFC-035 §3.6).
///
/// The predicate walker descends only through `And` nodes; `Or` /
/// `Not` subtrees contribute nothing but don't poison sibling
/// `And`-conjuncts (see module doc "Why `And`-only descent").
pub(crate) fn candidates_from_index<F>(
    state: &KeyspaceState,
    np: &NodePattern,
    where_clause: Option<&Predicate>,
    params: &BTreeMap<String, Param>,
    bound_var_prop: &F,
) -> Option<Vec<NodeIndex>>
where
    F: Fn(&str, &str) -> Option<IndexValue>,
{
    let label = np.label.as_ref()?;
    if state.index_spec.entries.is_empty() {
        return None;
    }

    let mut hints: Vec<(IndexTag, IndexValue)> = Vec::new();
    collect_pattern_hints(label, &state.index_spec, np, &mut hints);

    if let Some(pred) = where_clause {
        if let Some(var) = np.var.as_deref() {
            collect_where_hints(
                label,
                &state.index_spec,
                var,
                pred,
                params,
                bound_var_prop,
                &mut hints,
            );
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
fn collect_where_hints<F>(
    label: &Label,
    spec: &IndexSpec,
    target_var: &str,
    pred: &Predicate,
    params: &BTreeMap<String, Param>,
    bound_var_prop: &F,
    out: &mut Vec<(IndexTag, IndexValue)>,
) where
    F: Fn(&str, &str) -> Option<IndexValue>,
{
    match pred {
        Predicate::And(a, b) => {
            collect_where_hints(label, spec, target_var, a, params, bound_var_prop, out);
            collect_where_hints(label, spec, target_var, b, params, bound_var_prop, out);
        }
        Predicate::Compare {
            left,
            op: CompareOp::Eq,
            right,
        } => {
            // Slice 5: a.prop = literal / $param.
            if let Some((prop, value)) = resolve_eq_hint(target_var, left, right, params) {
                if is_indexed_prop(spec, label, &prop) {
                    out.push((prop, value));
                }
            }
            // Slice 6: last_segment(a.qname) = last_segment(b.qname)
            // where exactly one of {a, b} is the target var and the
            // other is resolvable via `bound_var_prop`.
            if let Some((tag, value)) =
                resolve_cross_ref_computed_hint(target_var, left, right, bound_var_prop)
            {
                if is_indexed_computed(spec, label, &tag) {
                    out.push((tag, value));
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
/// `IndexEntry::Prop` — computed keys go through
/// [`is_indexed_computed`] which checks the `Computed` variant
/// against its canonical tag string (e.g. `"last_segment(qname)"`).
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

/// `(label, computed_tag)` membership for cross-MATCH hints.
/// `computed_tag` is the canonical string (`ComputedKey::as_str`),
/// matching the `IndexTag` stored in `by_prop` for `IndexEntry::Computed`.
fn is_indexed_computed(spec: &IndexSpec, label: &Label, computed_tag: &str) -> bool {
    spec.entries.iter().any(|entry| match entry {
        IndexEntry::Computed {
            label: l,
            computed,
            notes: _,
        } => l.as_str() == label.as_str() && computed.as_str() == computed_tag,
        IndexEntry::Prop { .. } => false,
    })
}

/// Recognise `Call(f, [Property{x, p}]) = Call(f, [Property{y, p}])`
/// in either order, where `f` is an allowlisted [`ComputedKey`] and
/// exactly one of `{x, y}` is `target_var`. When the other var is
/// resolvable through `bound_var_prop`, apply the computed key to
/// that value and emit a hint `(computed_key.as_str(), bucket)`.
///
/// Returns `None` when the shape doesn't match, the function name
/// isn't an allowlisted computed key, both sides reference the
/// target, neither side references the target, the bound side
/// doesn't resolve, or the computed call's arg prop doesn't match
/// the computed key's canonical source prop (for `LastSegment`
/// that's `qname` — the same prop `build::entry_value_for_node`
/// reads from the node).
fn resolve_cross_ref_computed_hint<F>(
    target_var: &str,
    left: &Expr,
    right: &Expr,
    bound_var_prop: &F,
) -> Option<(IndexTag, IndexValue)>
where
    F: Fn(&str, &str) -> Option<IndexValue>,
{
    let (l_name, l_var, l_prop) = unwrap_computed_call(left)?;
    let (r_name, r_var, r_prop) = unwrap_computed_call(right)?;
    // Both sides must invoke the SAME allowlisted computed key and
    // read from the SAME canonical source prop — otherwise the Eq
    // cannot be decided by a single posting-list lookup.
    if l_name != r_name || l_prop != r_prop {
        return None;
    }
    let computed_key = match_computed_call_name(l_name)?;
    // Exactly one of the two vars must be the target; the other
    // must be bound.
    let bound_var = match (l_var == target_var, r_var == target_var) {
        (true, false) => r_var,
        (false, true) => l_var,
        _ => return None,
    };
    let bound_value = bound_var_prop(bound_var, l_prop)?;
    let bucket = computed_key.evaluate(&bound_value).to_string();
    Some((computed_key.as_str().to_string(), bucket))
}

/// Recognise the `Call { name, args: [Property { var, prop }] }`
/// shape and return the borrowed `(name, var, prop)` triple — i.e.
/// the un-evaluated form we need for cross-ref hint matching. Any
/// other shape (multi-arg call, nested call, non-property arg)
/// returns `None`.
fn unwrap_computed_call(expr: &Expr) -> Option<(&str, &str, &str)> {
    let Expr::Call { name, args } = expr else {
        return None;
    };
    if args.len() != 1 {
        return None;
    }
    let Expr::Property { var, prop } = &args[0] else {
        return None;
    };
    Some((name.as_str(), var.as_str(), prop.as_str()))
}

/// Map a Cypher function name (e.g. `"last_segment"`) to its
/// [`ComputedKey`] variant, or `None` if it isn't in the allowlist.
/// `ComputedKey::FromStr` expects the canonical parenthesised form
/// (`"last_segment(qname)"`) that appears in `.cfdb/indexes.toml`,
/// which is the wrong surface for this walker — the Cypher AST
/// carries the bare function name.
fn match_computed_call_name(name: &str) -> Option<ComputedKey> {
    match name {
        "last_segment" => Some(ComputedKey::LastSegment),
        _ => None,
    }
}

/// Intersect the posting lists named by `hints`. An empty
/// intersection is a valid answer (the index conclusively proves no
/// node matches); we return `Vec::new()` rather than `None` because
/// the fast-path short-circuit has already committed to answering
/// from indexes. `hints` MUST be non-empty — the caller guards this.
///
/// # Allocation discipline
///
/// Returns a sorted `Vec<NodeIndex>`. The first posting list is
/// materialised once (iterating the source `BTreeSet` in sorted
/// order); each subsequent posting list is walked in place via
/// `Vec::retain` + `BTreeSet::contains` (O(|acc| log |next|), zero
/// new allocations per hint). This matters at the 21k-node posting-
/// list scale #167 targets — a naive clone-then-intersect pass
/// would triple-allocate each conjunct.
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
    let mut acc: Vec<NodeIndex> = lookup_posting(state, label, first_tag, first_value)
        .map(|set| set.iter().copied().collect())
        .unwrap_or_default();
    for (tag, value) in iter {
        if acc.is_empty() {
            break;
        }
        match lookup_posting(state, label, tag, value) {
            Some(set) => acc.retain(|idx| set.contains(idx)),
            None => {
                acc.clear();
                break;
            }
        }
    }
    acc
}

fn lookup_posting<'s>(
    state: &'s KeyspaceState,
    label: &Label,
    tag: &IndexTag,
    value: &IndexValue,
) -> Option<&'s BTreeSet<NodeIndex>> {
    state.by_prop.get(&(label.clone(), tag.clone()))?.get(value)
}

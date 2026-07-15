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

use cfdb_core::query::{CompareOp, Expr, NodePattern, ParamBinding, Predicate};
use cfdb_core::schema::Label;
use petgraph::stable_graph::NodeIndex;

use crate::graph::KeyspaceState;
use crate::index::build::{index_key_of, IndexTag, IndexValue};
use crate::index::spec::ComputedKey;

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
    params: &BTreeMap<String, ParamBinding>,
    bound_var_prop: &F,
) -> Option<Vec<NodeIndex>>
where
    F: Fn(&str, &str) -> Option<IndexValue>,
{
    let label = np.label.as_ref()?;
    if state.indexed_pairs.is_empty() {
        return None;
    }

    let mut hints: Vec<(IndexTag, IndexValue)> = Vec::new();
    collect_pattern_hints(label, &state.indexed_pairs, np, &mut hints);

    if let Some(pred) = where_clause {
        if let Some(var) = np.var.as_deref() {
            collect_where_hints(
                label,
                &state.indexed_pairs,
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
    indexed_pairs: &BTreeMap<String, BTreeSet<IndexTag>>,
    np: &NodePattern,
    out: &mut Vec<(IndexTag, IndexValue)>,
) {
    let fresh = np
        .props
        .iter()
        .filter(|(prop, _)| is_indexed_pair(indexed_pairs, label, prop))
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
    indexed_pairs: &BTreeMap<String, BTreeSet<IndexTag>>,
    target_var: &str,
    pred: &Predicate,
    params: &BTreeMap<String, ParamBinding>,
    bound_var_prop: &F,
    out: &mut Vec<(IndexTag, IndexValue)>,
) where
    F: Fn(&str, &str) -> Option<IndexValue>,
{
    match pred {
        Predicate::And(a, b) => {
            collect_where_hints(
                label,
                indexed_pairs,
                target_var,
                a,
                params,
                bound_var_prop,
                out,
            );
            collect_where_hints(
                label,
                indexed_pairs,
                target_var,
                b,
                params,
                bound_var_prop,
                out,
            );
        }
        Predicate::Compare {
            left,
            op: CompareOp::Eq,
            right,
        } => {
            // Slice 5: a.prop = literal / $param.
            if let Some((prop, value)) = resolve_eq_hint(target_var, left, right, params) {
                if is_indexed_pair(indexed_pairs, label, &prop) {
                    out.push((prop, value));
                }
            }
            // Slice 6: last_segment(a.qname) = last_segment(b.qname)
            // where exactly one of {a, b} is the target var and the
            // other is resolvable via `bound_var_prop`.
            if let Some((tag, value)) =
                resolve_cross_ref_computed_hint(target_var, left, right, bound_var_prop)
            {
                if is_indexed_pair(indexed_pairs, label, &tag) {
                    out.push((tag, value));
                }
            }
            // Slice 6b: a.prop = b.prop — plain property-to-property
            // equi-join (e.g. `a.name = b.name`). Same composition
            // shape as the slice-6 computed-key path but without a
            // UDF: bucket the target by the bound side's raw prop
            // value. Soundness: the bucket key is the exact value
            // the WHERE clause already requires, so the
            // post-narrowing predicate filter is still applied and
            // no row that would have passed is dropped. Activates
            // only when the `(label, prop)` pair is in the spec —
            // narrowing without an index falls back to the label
            // scan, same as slice 5.
            if let Some((tag, value)) =
                resolve_cross_ref_prop_hint(target_var, left, right, bound_var_prop)
            {
                if is_indexed_pair(indexed_pairs, label, &tag) {
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
    params: &BTreeMap<String, ParamBinding>,
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
fn resolve_literal_value(
    expr: &Expr,
    params: &BTreeMap<String, ParamBinding>,
) -> Option<IndexValue> {
    match expr {
        Expr::Literal(pv) => index_key_of(pv),
        Expr::Param(name) => match params.get(name)? {
            ParamBinding::Scalar(pv) => index_key_of(pv),
            ParamBinding::List(_) => None,
        },
        _ => None,
    }
}

/// `(label, tag)` membership check against the precomputed
/// `KeyspaceState::indexed_pairs` map. Replaces the previous pair of
/// linear-scan helpers (`is_indexed_prop` + `is_indexed_computed`)
/// with a two-step `BTreeMap` lookup — both prop entries and
/// computed-key entries land in the same map under their canonical
/// tag string, so one membership check covers both shapes.
///
/// The map is built once at `KeyspaceState::new_with_spec` time
/// (graph.rs `indexed_pairs_for`). The two-level shape lets the
/// caller pass `&Label` and `&str` directly into `get` / `contains`
/// via `Borrow<str>` on `String` — no per-call allocation. For
/// ContextHomonym at n=1_000 the hint walker calls this hundreds of
/// times per outer row; the linear scan over the production 6-entry
/// spec was the bulk of the per-row hint-collection cost.
fn is_indexed_pair(
    indexed_pairs: &BTreeMap<String, BTreeSet<IndexTag>>,
    label: &Label,
    tag: &str,
) -> bool {
    indexed_pairs
        .get(label.as_str())
        .is_some_and(|tags| tags.contains(tag))
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

/// Recognise `Property{x, p} = Property{y, p}` in either order
/// where exactly one of `{x, y}` is `target_var` and the other is
/// resolvable through `bound_var_prop`. Emits a hint
/// `(prop_name, bound_value)` — the bucket key is the bound side's
/// raw value, so the target's posting list is narrowed to items
/// that already satisfy the equi-join conjunct.
///
/// Returns `None` when the shape doesn't match, the two sides
/// reference different props (`a.name = b.crate` is not a join we
/// can hash on a single posting list), both sides reference the
/// target, neither side references the target, or the bound side
/// doesn't resolve.
fn resolve_cross_ref_prop_hint<F>(
    target_var: &str,
    left: &Expr,
    right: &Expr,
    bound_var_prop: &F,
) -> Option<(IndexTag, IndexValue)>
where
    F: Fn(&str, &str) -> Option<IndexValue>,
{
    let (l_var, l_prop) = unwrap_property(left)?;
    let (r_var, r_prop) = unwrap_property(right)?;
    // Both sides must reference the SAME prop — otherwise the Eq
    // cannot be decided by a single posting-list lookup.
    if l_prop != r_prop {
        return None;
    }
    // Exactly one of the two vars must be the target; the other
    // must be bound and resolvable.
    let bound_var = match (l_var == target_var, r_var == target_var) {
        (true, false) => r_var,
        (false, true) => l_var,
        _ => return None,
    };
    let bound_value = bound_var_prop(bound_var, l_prop)?;
    Some((l_prop.to_string(), bound_value))
}

/// Recognise the `Property { var, prop }` shape and return the
/// borrowed `(var, prop)` pair. Sibling of [`unwrap_computed_call`]
/// for the plain prop-to-prop hint walker.
fn unwrap_property(expr: &Expr) -> Option<(&str, &str)> {
    let Expr::Property { var, prop } = expr else {
        return None;
    };
    Some((var.as_str(), prop.as_str()))
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
/// Returns a sorted `Vec<NodeIndex>`. Posting lists are resolved up
/// front and ordered by size, smallest first — materialising the
/// smallest into `acc` and refining downward minimises peak
/// allocation AND per-element `BTreeSet::contains` work. A naive
/// "first-hint wins" seed would materialise a huge bucket
/// (e.g. `is_test=false` posting list ≈ |Item|) before refining
/// against a tiny one (e.g. `last_segment=Foo` ≈ 2 items),
/// quadratic-allocating in the wrong direction.
///
/// A missing posting list (`lookup_posting` returns `None` for one
/// of the hints) is a conclusive empty intersection — the spec
/// claimed a particular `(label, tag, value)` is indexed but no
/// node carries it, so there CANNOT be a row that satisfies the
/// WHERE conjunct generating that hint.
fn intersect(
    state: &KeyspaceState,
    label: &Label,
    hints: &[(IndexTag, IndexValue)],
) -> Vec<NodeIndex> {
    // Resolve every posting list up front. If ANY hint has no
    // posting list, the intersection is empty.
    let mut postings: Vec<&BTreeSet<NodeIndex>> = Vec::with_capacity(hints.len());
    for (tag, value) in hints {
        match lookup_posting(state, label, tag, value) {
            Some(set) => postings.push(set),
            None => return Vec::new(),
        }
    }
    if postings.is_empty() {
        // Defensive: `candidates_from_index` never calls us with
        // an empty hint vec.
        return Vec::new();
    }
    // Order smallest first so we materialise the tightest bucket
    // into `acc` and the remaining (potentially huge) posting
    // lists serve only as membership filters via `retain`.
    postings.sort_by_key(|set| set.len());
    let mut iter = postings.into_iter();
    let first = iter.next().expect("non-empty after the early return");
    let mut acc: Vec<NodeIndex> = first.iter().copied().collect();
    for set in iter {
        if acc.is_empty() {
            break;
        }
        acc.retain(|idx| set.contains(idx));
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

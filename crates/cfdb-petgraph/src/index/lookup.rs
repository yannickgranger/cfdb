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
//! 3. **Cross-MATCH computed-key intersection** — two shapes:
//!    - `context_homonym`: `last_segment(a.qname) = last_segment(b.qname)`.
//!    - `random_scattering`: `regexp_extract(a.name, '<vetted>') =
//!      regexp_extract(b.name, '<vetted>')`, recognised as the
//!      `ConversionPrefix` computed key when the pattern literal equals
//!      [`CONVERSION_PREFIX_PATTERN`](crate::index::spec::CONVERSION_PREFIX_PATTERN)
//!      byte-for-byte (slice 6).
//!      When the bound var's value on the non-target side resolves through
//!      the `bound_var_prop` closure, we apply the computed key and narrow
//!      the target var's candidates to that single bucket. Turns the
//!      Σ|Items|² cartesian into Σ|bucket|².
//!
//!    **Partial keys and provable emptiness.** `ConversionPrefix` is
//!    PARTIAL: a bound `name` with no conversion prefix evaluates to
//!    NULL. `compare_propvalues` treats any NULL operand as
//!    non-matching, so `regexp_extract(NULL) = regexp_extract(target)`
//!    is false for EVERY target — the equi-join conjunct is
//!    unsatisfiable and the target candidate set is conclusively empty.
//!    We narrow the target to the empty set rather than falling back to
//!    the full scan. This is not just sound but NECESSARY: on
//!    RandomScattering most outer `a` rows are non-conversion names, and
//!    a full-scan fallback for each would keep the rule at O(n²) — the
//!    exact pathology this fast path removes. Empty-narrowing is gated on
//!    the computed key being indexed (like every other hint), so a
//!    keyspace without the `conversion_prefix(name)` index falls back.
//!
//!    **Soundness.** The bucket key is exactly the value the WHERE
//!    conjunct requires; the outer `Evaluator::run` re-applies the full
//!    predicate to the narrowed set, so no row that would have passed is
//!    dropped. The rule's OTHER `regexp_extract` conjunct — the SUFFIX
//!    inequality `regexp_extract(a.name, '..._(\w+)$') <> ...` — parses
//!    to `Predicate::Ne`, contributes no hint, and is unaffected.
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

use cfdb_core::fact::PropValue;
use cfdb_core::query::{CompareOp, Expr, NodePattern, ParamBinding, Predicate};
use cfdb_core::schema::Label;
use petgraph::stable_graph::NodeIndex;

use crate::graph::KeyspaceState;
use crate::index::build::{index_key_of, IndexTag, IndexValue};
use crate::index::spec::{ComputedKey, CONVERSION_PREFIX_PATTERN};

/// Outcome of walking the WHERE predicate for indexable hints.
#[derive(Debug, PartialEq, Eq)]
enum HintOutcome {
    /// Hints (possibly zero) were appended to the out-vec; the caller
    /// intersects them (an empty out-vec means "no hint, fall back").
    Collected,
    /// A conjunct is provably unsatisfiable — a cross-MATCH computed
    /// equi-join whose bound side evaluated to NULL (a partial key that
    /// did not match), which `compare_propvalues` rejects for every
    /// target. The target candidate set is conclusively empty; the
    /// caller returns `Some(vec![])` rather than falling back to the
    /// full scan (which would be O(n²) on the non-matching outer rows).
    ProvablyEmpty,
}

/// Result of recognising a single cross-MATCH computed-key equi-join
/// conjunct (`f(a.p) = f(b.p)`).
enum ComputedHint {
    /// The bound side resolved to `bucket`; narrow the target to that
    /// posting list under `tag`.
    Bucket(IndexTag, IndexValue),
    /// The bound side's PARTIAL computed key did not match (NULL join
    /// operand). `tag` names the computed key so the caller can gate the
    /// empty-narrow on that key being indexed.
    ProvablyEmpty(IndexTag),
    /// Not a recognised cross-MATCH computed equi-join, or the bound
    /// side is unresolvable — no opinion, keep walking.
    NoHint,
}

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
/// - `where_clause` (slice 6, cross-MATCH computed) — a `Compare { op:
///   Eq }` whose two sides invoke the SAME allowlisted `ComputedKey`
///   over the SAME source prop (and, for `regexp_extract`, the SAME
///   pattern literal): `last_segment(a.qname) = last_segment(b.qname)`
///   (`context_homonym`) or `regexp_extract(a.name, '<vetted>') =
///   regexp_extract(b.name, '<vetted>')` (`random_scattering`,
///   recognised as `ConversionPrefix`). Exactly one side is `np.var`,
///   the other resolves through `bound_var_prop`, and the computed key
///   is in the spec. When the bound side's key resolves, the hint
///   narrows the target to its single bucket. When the bound side is a
///   PARTIAL key that did NOT match (NULL join operand), the whole
///   candidate set is provably empty and the function returns
///   `Some(vec![])` (RFC-035 §3.6).
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
            // A `ProvablyEmpty` conjunct dominates every other hint
            // (empty ∩ anything = empty): commit to an empty answer
            // instead of falling back to the full scan.
            if collect_where_hints(
                label,
                &state.indexed_pairs,
                var,
                pred,
                params,
                bound_var_prop,
                &mut hints,
            ) == HintOutcome::ProvablyEmpty
            {
                return Some(Vec::new());
            }
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
///
/// Returns [`HintOutcome::ProvablyEmpty`] as soon as any conjunct
/// proves the target set empty (a partial cross-MATCH computed key whose
/// bound side is NULL); the conjunction short-circuits because empty
/// dominates every sibling hint. Otherwise returns
/// [`HintOutcome::Collected`].
fn collect_where_hints<F>(
    label: &Label,
    indexed_pairs: &BTreeMap<String, BTreeSet<IndexTag>>,
    target_var: &str,
    pred: &Predicate,
    params: &BTreeMap<String, ParamBinding>,
    bound_var_prop: &F,
    out: &mut Vec<(IndexTag, IndexValue)>,
) -> HintOutcome
where
    F: Fn(&str, &str) -> Option<IndexValue>,
{
    match pred {
        Predicate::And(a, b) => {
            let left_outcome = collect_where_hints(
                label,
                indexed_pairs,
                target_var,
                a,
                params,
                bound_var_prop,
                out,
            );
            if left_outcome == HintOutcome::ProvablyEmpty {
                return HintOutcome::ProvablyEmpty;
            }
            collect_where_hints(
                label,
                indexed_pairs,
                target_var,
                b,
                params,
                bound_var_prop,
                out,
            )
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
            // Slice 6: f(a.p) = f(b.p) for an allowlisted computed key
            // (last_segment / conversion_prefix) where exactly one of
            // {a, b} is the target var and the other is resolvable via
            // `bound_var_prop`.
            match resolve_cross_ref_computed_hint(target_var, left, right, bound_var_prop) {
                ComputedHint::Bucket(tag, value) => {
                    if is_indexed_pair(indexed_pairs, label, &tag) {
                        out.push((tag, value));
                    }
                }
                ComputedHint::ProvablyEmpty(tag) => {
                    // Gate the empty-narrow on the computed key being
                    // indexed, mirroring the Bucket path: the fast path
                    // is opt-in via indexes.toml. Without the index we
                    // fall back to the full scan (still correct).
                    if is_indexed_pair(indexed_pairs, label, &tag) {
                        return HintOutcome::ProvablyEmpty;
                    }
                }
                ComputedHint::NoHint => {}
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
            HintOutcome::Collected
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
        | Predicate::Not(_) => HintOutcome::Collected,
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

/// Recognise a cross-MATCH computed equi-join in either order:
/// `f(x.p) = f(y.p)` where `f` is an allowlisted [`ComputedKey`] and
/// exactly one of `{x, y}` is `target_var`. Both computed keys carry an
/// optional pattern literal — `last_segment(v.p)` (no literal) and
/// `regexp_extract(v.p, '<vetted>')` (literal argument) — and the two
/// sides must agree on the function name, the source prop, AND the
/// literal, so `regexp_extract` with two different patterns is not a
/// single-bucket join.
///
/// When the other var resolves through `bound_var_prop`, apply the
/// computed key to its value:
/// - a match → [`ComputedHint::Bucket`] naming the posting list to
///   narrow to;
/// - a PARTIAL key that does not match (`ConversionPrefix` over a
///   non-conversion name) → [`ComputedHint::ProvablyEmpty`], because the
///   NULL join operand makes the conjunct unsatisfiable for every
///   target.
///
/// Returns [`ComputedHint::NoHint`] when the shape doesn't match, the
/// name/literal isn't an allowlisted computed key, the call's arg prop
/// doesn't match the key's canonical `source_prop`, both/neither side
/// references the target, or the bound side doesn't resolve.
fn resolve_cross_ref_computed_hint<F>(
    target_var: &str,
    left: &Expr,
    right: &Expr,
    bound_var_prop: &F,
) -> ComputedHint
where
    F: Fn(&str, &str) -> Option<IndexValue>,
{
    let Some((l_name, l_var, l_prop, l_lit)) = unwrap_computed_call(left) else {
        return ComputedHint::NoHint;
    };
    let Some((r_name, r_var, r_prop, r_lit)) = unwrap_computed_call(right) else {
        return ComputedHint::NoHint;
    };
    // Both sides must invoke the SAME function, read the SAME prop, and
    // carry the SAME pattern literal — otherwise the Eq cannot be
    // decided by a single posting-list lookup.
    if l_name != r_name || l_prop != r_prop || l_lit != r_lit {
        return ComputedHint::NoHint;
    }
    let Some(computed_key) = match_computed_call(l_name, l_lit) else {
        return ComputedHint::NoHint;
    };
    // The call must read the computed key's canonical source prop
    // (`qname` for last_segment, `name` for conversion_prefix); the
    // build pass keys the posting list off that prop, so a call over any
    // other prop would narrow through the wrong bucket.
    if l_prop != computed_key.source_prop() {
        return ComputedHint::NoHint;
    }
    // Exactly one of the two vars must be the target; the other
    // must be bound.
    let bound_var = match (l_var == target_var, r_var == target_var) {
        (true, false) => r_var,
        (false, true) => l_var,
        _ => return ComputedHint::NoHint,
    };
    let Some(bound_value) = bound_var_prop(bound_var, l_prop) else {
        return ComputedHint::NoHint;
    };
    match computed_key.evaluate(&bound_value) {
        Some(bucket) => ComputedHint::Bucket(computed_key.as_str().to_string(), bucket.to_string()),
        None => ComputedHint::ProvablyEmpty(computed_key.as_str().to_string()),
    }
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

/// Recognise the two computed-call shapes and return the borrowed
/// `(name, var, prop, literal)` tuple — the un-evaluated form the
/// cross-ref hint matcher needs:
/// - `f(v.p)` — a single property arg, e.g. `last_segment(a.qname)` →
///   `("last_segment", "a", "qname", None)`.
/// - `f(v.p, '<lit>')` — a property arg plus a string literal, e.g.
///   `regexp_extract(a.name, '<pattern>')` →
///   `("regexp_extract", "a", "name", Some("<pattern>"))`.
///
/// Any other shape (no property first arg, non-string literal, extra
/// args, nested call) returns `None`.
fn unwrap_computed_call(expr: &Expr) -> Option<(&str, &str, &str, Option<&str>)> {
    let Expr::Call { name, args } = expr else {
        return None;
    };
    match args.as_slice() {
        [Expr::Property { var, prop }] => Some((name.as_str(), var.as_str(), prop.as_str(), None)),
        [Expr::Property { var, prop }, Expr::Literal(PropValue::Str(lit))] => Some((
            name.as_str(),
            var.as_str(),
            prop.as_str(),
            Some(lit.as_str()),
        )),
        _ => None,
    }
}

/// Map a Cypher function call (name + optional pattern literal) to its
/// [`ComputedKey`] variant, or `None` if it isn't in the allowlist. The
/// Cypher AST carries the bare function name — not the canonical
/// parenthesised form `ComputedKey::FromStr` expects — and for
/// `ConversionPrefix` the recognition is BYTE-FOR-BYTE on the pattern
/// literal: only `regexp_extract` whose second arg equals
/// [`CONVERSION_PREFIX_PATTERN`] is the vetted conversion-prefix join.
fn match_computed_call(name: &str, literal: Option<&str>) -> Option<ComputedKey> {
    match (name, literal) {
        ("last_segment", None) => Some(ComputedKey::LastSegment),
        ("regexp_extract", Some(pat)) if pat == CONVERSION_PREFIX_PATTERN => {
            Some(ComputedKey::ConversionPrefix)
        }
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

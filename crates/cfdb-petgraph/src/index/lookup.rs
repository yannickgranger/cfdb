use std::collections::{BTreeMap, BTreeSet};

use cfdb_core::fact::PropValue;
use cfdb_core::query::{CompareOp, Expr, NodePattern, ParamBinding, Predicate};
use cfdb_core::schema::Label;
use petgraph::stable_graph::NodeIndex;

use crate::graph::KeyspaceState;
use crate::index::build::{index_key_of, IndexTag, IndexValue};
use crate::index::spec::{ComputedKey, CONVERSION_PREFIX_PATTERN};

#[derive(Debug, PartialEq, Eq)]
enum HintOutcome {
    Collected,
    ProvablyEmpty,
}

enum ComputedHint {
    Bucket(IndexTag, IndexValue),
    ProvablyEmpty(IndexTag),
    NoHint,
}

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
            if let Some((prop, value)) = resolve_eq_hint(target_var, left, right, params) {
                if is_indexed_pair(indexed_pairs, label, &prop) {
                    out.push((prop, value));
                }
            }
            match resolve_cross_ref_computed_hint(target_var, left, right, bound_var_prop) {
                ComputedHint::Bucket(tag, value) => {
                    if is_indexed_pair(indexed_pairs, label, &tag) {
                        out.push((tag, value));
                    }
                }
                ComputedHint::ProvablyEmpty(tag) => {
                    if is_indexed_pair(indexed_pairs, label, &tag) {
                        return HintOutcome::ProvablyEmpty;
                    }
                }
                ComputedHint::NoHint => {}
            }
            if let Some((tag, value)) =
                resolve_cross_ref_prop_hint(target_var, left, right, bound_var_prop)
            {
                if is_indexed_pair(indexed_pairs, label, &tag) {
                    out.push((tag, value));
                }
            }
            HintOutcome::Collected
        }
        Predicate::Compare { .. }
        | Predicate::In { .. }
        | Predicate::Regex { .. }
        | Predicate::NotExists { .. }
        | Predicate::Ne { .. }
        | Predicate::Or(_, _)
        | Predicate::Not(_) => HintOutcome::Collected,
    }
}

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

fn is_indexed_pair(
    indexed_pairs: &BTreeMap<String, BTreeSet<IndexTag>>,
    label: &Label,
    tag: &str,
) -> bool {
    indexed_pairs
        .get(label.as_str())
        .is_some_and(|tags| tags.contains(tag))
}

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
    if l_name != r_name || l_prop != r_prop || l_lit != r_lit {
        return ComputedHint::NoHint;
    }
    let Some(computed_key) = match_computed_call(l_name, l_lit) else {
        return ComputedHint::NoHint;
    };
    if l_prop != computed_key.source_prop() {
        return ComputedHint::NoHint;
    }
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
    if l_prop != r_prop {
        return None;
    }
    let bound_var = match (l_var == target_var, r_var == target_var) {
        (true, false) => r_var,
        (false, true) => l_var,
        _ => return None,
    };
    let bound_value = bound_var_prop(bound_var, l_prop)?;
    Some((l_prop.to_string(), bound_value))
}

fn unwrap_property(expr: &Expr) -> Option<(&str, &str)> {
    let Expr::Property { var, prop } = expr else {
        return None;
    };
    Some((var.as_str(), prop.as_str()))
}

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

fn match_computed_call(name: &str, literal: Option<&str>) -> Option<ComputedKey> {
    match (name, literal) {
        ("last_segment", None) => Some(ComputedKey::LastSegment),
        ("regexp_extract", Some(pat)) if pat == CONVERSION_PREFIX_PATTERN => {
            Some(ComputedKey::ConversionPrefix)
        }
        _ => None,
    }
}

fn intersect(
    state: &KeyspaceState,
    label: &Label,
    hints: &[(IndexTag, IndexValue)],
) -> Vec<NodeIndex> {
    let mut postings: Vec<&BTreeSet<NodeIndex>> = Vec::with_capacity(hints.len());
    for (tag, value) in hints {
        match lookup_posting(state, label, tag, value) {
            Some(set) => postings.push(set),
            None => return Vec::new(),
        }
    }
    if postings.is_empty() {
        return Vec::new();
    }
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

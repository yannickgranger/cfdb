//! Pure helpers for projection aliasing, deterministic sort keys, value
//! comparison, and did-you-mean label suggestions.
//!
//! Everything here is stateless — no reference to `Evaluator`.

use cfdb_core::fact::PropValue;
use cfdb_core::query::{Aggregation, Expr, Projection, ProjectionValue};
use cfdb_core::result::{Row, RowValue};

pub(super) fn projection_alias(proj: &Projection) -> String {
    if let Some(alias) = &proj.alias {
        return alias.clone();
    }
    match &proj.value {
        ProjectionValue::Expr(e) => expr_alias(e),
        ProjectionValue::Aggregation(a) => aggregation_alias(a),
    }
}

pub(super) fn expr_alias(expr: &Expr) -> String {
    match expr {
        Expr::Property { var, prop } => format!("{}.{}", var, prop),
        Expr::Var(v) => v.clone(),
        Expr::Literal(_) => "literal".to_string(),
        Expr::Param(p) => format!("${}", p),
        Expr::List(_) => "list".to_string(),
        Expr::Call { name, .. } => name.clone(),
    }
}

fn aggregation_alias(agg: &Aggregation) -> String {
    match agg {
        Aggregation::CountStar => "count".to_string(),
        Aggregation::Count(_) => "count".to_string(),
        Aggregation::CountDistinct(_) => "count_distinct".to_string(),
        Aggregation::Collect(_) => "collect".to_string(),
        Aggregation::CollectDistinct(_) => "collect_distinct".to_string(),
        Aggregation::Size(_) => "size".to_string(),
    }
}

pub(super) fn row_value_cmp(a: &RowValue, b: &RowValue) -> std::cmp::Ordering {
    match (a, b) {
        (RowValue::Scalar(x), RowValue::Scalar(y)) => propvalue_cmp(x, y),
        (RowValue::List(x), RowValue::List(y)) => x.len().cmp(&y.len()),
        (RowValue::Scalar(_), RowValue::List(_)) => std::cmp::Ordering::Less,
        (RowValue::List(_), RowValue::Scalar(_)) => std::cmp::Ordering::Greater,
    }
}

fn propvalue_cmp(a: &PropValue, b: &PropValue) -> std::cmp::Ordering {
    match (a, b) {
        (PropValue::Null, PropValue::Null) => std::cmp::Ordering::Equal,
        (PropValue::Null, _) => std::cmp::Ordering::Less,
        (_, PropValue::Null) => std::cmp::Ordering::Greater,
        (PropValue::Int(x), PropValue::Int(y)) => x.cmp(y),
        (PropValue::Float(x), PropValue::Float(y)) => {
            x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal)
        }
        (PropValue::Int(x), PropValue::Float(y)) => (*x as f64)
            .partial_cmp(y)
            .unwrap_or(std::cmp::Ordering::Equal),
        (PropValue::Float(x), PropValue::Int(y)) => x
            .partial_cmp(&(*y as f64))
            .unwrap_or(std::cmp::Ordering::Equal),
        (PropValue::Str(x), PropValue::Str(y)) => x.cmp(y),
        (PropValue::Bool(x), PropValue::Bool(y)) => x.cmp(y),
        _ => propvalue_sort_key(a).cmp(&propvalue_sort_key(b)),
    }
}

pub(super) fn propvalue_sort_key(v: &PropValue) -> String {
    match v {
        PropValue::Null => "0:".to_string(),
        PropValue::Bool(b) => format!("1:{}", b),
        PropValue::Int(i) => format!("2:{:020}", i),
        PropValue::Float(f) => format!("3:{:020.10}", f),
        PropValue::Str(s) => format!("4:{}", s),
    }
}

pub(super) fn row_sort_key(row: &Row) -> String {
    let mut parts: Vec<String> = Vec::with_capacity(row.len());
    for (k, v) in row {
        parts.push(format!("{}=", k));
        parts.push(match v {
            RowValue::Scalar(p) => propvalue_sort_key(p),
            RowValue::List(items) => {
                let joined: Vec<String> = items.iter().map(propvalue_sort_key).collect();
                format!("L[{}]", joined.join(","))
            }
        });
    }
    parts.join("|")
}

/// Return a did-you-mean suggestion for `query` picked from `known`, if any
/// entry is within edit distance 2. Deterministic: ties broken by sorted order.
pub(super) fn suggest_label<'a>(query: &str, known: impl Iterator<Item = &'a str>) -> Option<String> {
    let mut best: Option<(usize, String)> = None;
    for cand in known {
        let d = edit_distance(query, cand);
        if d <= 2 {
            match &best {
                None => best = Some((d, cand.to_string())),
                Some((bd, bs)) if d < *bd || (d == *bd && cand < bs.as_str()) => {
                    best = Some((d, cand.to_string()));
                }
                _ => {}
            }
        }
    }
    best.map(|(_, s)| format!("did you mean `{}`?", s))
}

fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.is_empty() {
        return b.len();
    }
    if b.is_empty() {
        return a.len();
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut curr: Vec<usize> = vec![0; b.len() + 1];
    for i in 1..=a.len() {
        curr[0] = i;
        for j in 1..=b.len() {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            curr[j] = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[b.len()]
}

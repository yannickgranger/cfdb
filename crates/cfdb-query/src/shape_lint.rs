//! Query-shape lint — catches known performance footguns before evaluation.
//!
//! Study 001 §4.2 measured that the textbook F1a Cartesian form —
//!
//! ```cypher
//! MATCH (a:Item), (b:Item)
//! WHERE regexp_extract(a.qname, '[^:]+$') = regexp_extract(b.qname, '[^:]+$')
//!   AND a <> b
//! ```
//!
//! — runs in 212s on LadybugDB and 5s on petgraph at 15k items, while the
//! equivalent aggregation form (F1b) runs in 4–86ms on the same data. No
//! planner pushes `f(a.p) = f(b.p)` into a hash join, so cfdb flags it at
//! shape time and suggests the aggregation rewrite instead.
//!
//! v0.1 scope: **one rule**. `lint_shape` walks the top-level `match_clauses`
//! and WHERE clause only — nested subqueries (`NOT EXISTS { ... }`) are not
//! walked by this pass.

use cfdb_core::{CompareOp, Expr, Label, Pattern, Predicate, Query};

/// A shape-level finding. Non-exhaustive so v0.2 can add more rules without
/// breaking downstream match arms.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShapeLint {
    /// F1a — Cartesian MATCH over two bindings of the same label combined
    /// with a function-equality predicate (`f(a.p) = f(b.p)`). This is O(n²)
    /// on the binding count and no backend planner will recover it.
    CartesianFunctionEquality {
        /// Human-readable description including the label and the measured
        /// study numbers.
        message: String,
        /// The canonical aggregation rewrite to present to the caller.
        suggestion: String,
    },
}

/// Scan a `Query` for known pathological shapes. Empty output means the query
/// is structurally acceptable — runtime complexity under the chosen backend
/// is still the caller's responsibility.
pub fn lint_shape(query: &Query) -> Vec<ShapeLint> {
    let mut out = Vec::new();

    if let Some(hit) = detect_cartesian_function_equality(query) {
        out.push(hit);
    }

    out
}

fn detect_cartesian_function_equality(query: &Query) -> Option<ShapeLint> {
    let same_label_pairs = collect_same_label_pairs(&query.match_clauses);
    if same_label_pairs.is_empty() {
        return None;
    }

    let where_clause = query.where_clause.as_ref()?;
    let label = find_function_equality_over_pair(where_clause, &same_label_pairs)?;

    Some(ShapeLint::CartesianFunctionEquality {
        message: format!(
            "Cartesian MATCH with function-equality predicate; O(n²) on {label} — \
             measured at 212s (lbug) / 5s (petgraph) on 15k items in study 001"
        ),
        suggestion: format!(
            "MATCH (a:{label}) WITH f(a.prop) AS key, collect(DISTINCT a.crate) AS crates \
             WHERE size(crates) > 1 RETURN key, crates"
        ),
    })
}

/// Find `(var_a, var_b, label)` triples where two top-level `Pattern::Node`
/// bindings share the same label. Only the top-level `match_clauses` are
/// considered — edges inside `Pattern::Path` and `Pattern::Optional` are
/// deliberately out of scope for v0.1.
fn collect_same_label_pairs(patterns: &[Pattern]) -> Vec<(String, String, Label)> {
    let mut bindings: Vec<(String, Label)> = Vec::new();
    for p in patterns {
        if let Pattern::Node(np) = p {
            if let (Some(var), Some(label)) = (np.var.as_ref(), np.label.as_ref()) {
                bindings.push((var.clone(), label.clone()));
            }
        }
    }

    let mut out = Vec::new();
    for i in 0..bindings.len() {
        for j in (i + 1)..bindings.len() {
            if bindings[i].1 == bindings[j].1 {
                out.push((
                    bindings[i].0.clone(),
                    bindings[j].0.clone(),
                    bindings[i].1.clone(),
                ));
            }
        }
    }
    out
}

/// Walk the predicate tree looking for an equality comparison whose both
/// sides are `Call(Property(var_a, ...))` / `Call(Property(var_b, ...))` —
/// and `var_a`, `var_b` are the two bindings of one of the same-label pairs.
///
/// Returns the shared label so the caller can include it in the message.
fn find_function_equality_over_pair(
    pred: &Predicate,
    pairs: &[(String, String, Label)],
) -> Option<Label> {
    match pred {
        Predicate::Compare {
            left,
            op: CompareOp::Eq,
            right,
        } => match_call_over_distinct_vars(left, right, pairs),
        Predicate::And(a, b) | Predicate::Or(a, b) => find_function_equality_over_pair(a, pairs)
            .or_else(|| find_function_equality_over_pair(b, pairs)),
        Predicate::Not(inner) => find_function_equality_over_pair(inner, pairs),
        _ => None,
    }
}

fn match_call_over_distinct_vars(
    left: &Expr,
    right: &Expr,
    pairs: &[(String, String, Label)],
) -> Option<Label> {
    let lvar = extract_call_over_property_var(left)?;
    let rvar = extract_call_over_property_var(right)?;
    if lvar == rvar {
        return None;
    }
    for (a, b, label) in pairs {
        if (a == &lvar && b == &rvar) || (a == &rvar && b == &lvar) {
            return Some(label.clone());
        }
    }
    None
}

/// `f(var.prop)` → `Some("var")`. Also recurses into nested calls so
/// `outer(inner(var.prop))` still fires.
fn extract_call_over_property_var(e: &Expr) -> Option<String> {
    match e {
        Expr::Call { args, .. } => {
            for a in args {
                if let Expr::Property { var, .. } = a {
                    return Some(var.clone());
                }
                if let Some(v) = extract_call_over_property_var(a) {
                    return Some(v);
                }
            }
            None
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;

    #[test]
    fn fires_on_canonical_f1a_cartesian() {
        let q = parse(
            r#"
            MATCH (a:Item), (b:Item)
            WHERE regexp_extract(a.qname, '[^:]+$') = regexp_extract(b.qname, '[^:]+$')
            RETURN count(*) AS n
            "#,
        )
        .expect("parses");
        let hits = lint_shape(&q);
        assert_eq!(hits.len(), 1, "expected one lint hit, got {hits:?}");
        match &hits[0] {
            ShapeLint::CartesianFunctionEquality { message, .. } => {
                assert!(message.contains("Item"), "message: {message}");
                assert!(message.contains("212s"), "message: {message}");
            }
        }
    }

    #[test]
    fn silent_on_f1b_aggregation_form() {
        let q = parse(
            r#"
            MATCH (a:Item)
            WITH regexp_extract(a.qname, '[^:]+$') AS name,
                 collect(DISTINCT a.crate) AS crates
            WHERE size(crates) > 1
            RETURN count(*) AS n
            "#,
        )
        .expect("parses");
        assert!(lint_shape(&q).is_empty());
    }

    #[test]
    fn silent_on_simple_return() {
        let q = parse("MATCH (a:Item) RETURN a.qname").expect("parses");
        assert!(lint_shape(&q).is_empty());
    }

    #[test]
    fn silent_on_plain_property_equality_self_join() {
        // Plain `a.qname = b.qname` is acceptable — the backend can hash-join
        // on property equality. Only `f(a.p) = f(b.p)` is the footgun.
        let q = parse(
            r#"
            MATCH (a:Item), (b:Item)
            WHERE a.qname = b.qname
            RETURN count(*) AS n
            "#,
        )
        .expect("parses");
        assert!(lint_shape(&q).is_empty());
    }
}

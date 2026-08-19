use cfdb_core::{CompareOp, Expr, Label, Pattern, Predicate, Query};

#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShapeLint {
    CartesianFunctionEquality { message: String, suggestion: String },
}

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

fn collect_same_label_pairs(patterns: &[Pattern]) -> Vec<(String, String, Label)> {
    let bindings: Vec<(String, Label)> = patterns.iter().filter_map(pattern_node_binding).collect();

    let mut out = Vec::new();
    for i in 0..bindings.len() {
        emit_same_label_pairs_with(&bindings, i, &mut out);
    }
    out
}

fn pattern_node_binding(p: &Pattern) -> Option<(String, Label)> {
    let Pattern::Node(np) = p else {
        return None;
    };
    let var = np.var.as_ref()?;
    let label = np.label.as_ref()?;
    Some((var.clone(), label.clone()))
}

fn emit_same_label_pairs_with(
    bindings: &[(String, Label)],
    i: usize,
    out: &mut Vec<(String, String, Label)>,
) {
    let (i_var, i_label) = &bindings[i];
    bindings
        .iter()
        .enumerate()
        .skip(i + 1)
        .filter(|(_, (_, label))| label == i_label)
        .for_each(|(_, (j_var, _))| {
            out.push((i_var.clone(), j_var.clone(), i_label.clone()));
        });
}

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
    pairs
        .iter()
        .find(|(a, b, _)| (a == &lvar && b == &rvar) || (a == &rvar && b == &lvar))
        .map(|(_, _, label)| label.clone())
}

fn extract_call_over_property_var(e: &Expr) -> Option<String> {
    let Expr::Call { args, .. } = e else {
        return None;
    };
    args.iter().find_map(|a| match a {
        Expr::Property { var, .. } => Some(var.clone()),
        _ => extract_call_over_property_var(a),
    })
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

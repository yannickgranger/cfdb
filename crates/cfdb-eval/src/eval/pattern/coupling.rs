use cfdb_core::graph::{GraphReader, NodeHandle};
use cfdb_core::query::{EdgePattern, NodePattern, Pattern, Predicate};
use cfdb_core::result::RowValue;

use super::super::{Binding, Bindings};

pub(super) fn unwind_row(
    out: &mut Vec<Bindings>,
    bindings: &Bindings,
    items: &[cfdb_core::fact::PropValue],
    var: &str,
) {
    items.iter().for_each(|item| {
        let mut next = bindings.clone();
        next.insert(
            var.to_string(),
            Binding::Value(RowValue::Scalar(item.clone())),
        );
        out.push(next);
    });
}

pub(super) fn is_binding_independent_pattern<G: GraphReader + ?Sized>(
    np: &NodePattern,
    where_clause: Option<&Predicate>,
    state: &G,
) -> bool {
    let own_var: Option<&str> = np.var.as_deref();
    let label = match &np.label {
        Some(l) => l,
        None => {
            return match where_clause {
                None => true,
                Some(pred) => !predicate_couples_own_to_foreign(pred, own_var),
            };
        }
    };
    match where_clause {
        None => true,
        Some(pred) => !predicate_has_active_coupling(pred, own_var, label, state),
    }
}

pub(super) fn predicate_has_active_coupling<G: GraphReader + ?Sized>(
    pred: &Predicate,
    own_var: Option<&str>,
    label: &cfdb_core::schema::Label,
    state: &G,
) -> bool {
    match pred {
        Predicate::And(a, b) | Predicate::Or(a, b) => {
            predicate_has_active_coupling(a, own_var, label, state)
                || predicate_has_active_coupling(b, own_var, label, state)
        }
        Predicate::Not(inner) => predicate_has_active_coupling(inner, own_var, label, state),
        Predicate::Compare { left, right, .. }
        | Predicate::In { left, right }
        | Predicate::Ne { left, right } => {
            leaf_couples(left, right, own_var)
                && coupling_prop_is_populated(left, right, own_var, label, state)
        }
        Predicate::Regex { left, pattern } => {
            leaf_couples(left, pattern, own_var)
                && coupling_prop_is_populated(left, pattern, own_var, label, state)
        }
        Predicate::NotExists { .. } => true,
    }
}

pub(super) fn coupling_prop_is_populated<G: GraphReader + ?Sized>(
    a: &cfdb_core::query::Expr,
    b: &cfdb_core::query::Expr,
    own_var: Option<&str>,
    label: &cfdb_core::schema::Label,
    state: &G,
) -> bool {
    let Some(own) = own_var else {
        return true;
    };
    let mut tags: Vec<String> = Vec::new();
    collect_narrow_tags(a, own, &mut tags);
    collect_narrow_tags(b, own, &mut tags);
    if tags.is_empty() {
        return false;
    }
    tags.iter()
        .any(|tag| state.indexed_prop_is_populated(label, tag))
}

pub(super) fn collect_narrow_tags(
    expr: &cfdb_core::query::Expr,
    own_var: &str,
    out: &mut Vec<String>,
) {
    use cfdb_core::query::Expr as E;
    match expr {
        E::Property { var, prop } if var == own_var => {
            out.push(prop.clone());
        }
        E::Call { name, args } if args.len() == 1 => {
            if let E::Property { var, prop } = &args[0] {
                if var == own_var {
                    out.push(format!("{name}({prop})"));
                }
            }
        }
        _ => {}
    }
}

pub(super) fn predicate_couples_own_to_foreign(pred: &Predicate, own_var: Option<&str>) -> bool {
    match pred {
        Predicate::And(a, b) | Predicate::Or(a, b) => {
            predicate_couples_own_to_foreign(a, own_var)
                || predicate_couples_own_to_foreign(b, own_var)
        }
        Predicate::Not(inner) => predicate_couples_own_to_foreign(inner, own_var),
        Predicate::Compare { left, right, .. }
        | Predicate::In { left, right }
        | Predicate::Ne { left, right } => leaf_couples(left, right, own_var),
        Predicate::Regex { left, pattern } => leaf_couples(left, pattern, own_var),
        Predicate::NotExists { .. } => true,
    }
}

pub(super) fn leaf_couples(
    a: &cfdb_core::query::Expr,
    b: &cfdb_core::query::Expr,
    own_var: Option<&str>,
) -> bool {
    let mut vars: Vec<&str> = Vec::new();
    collect_expr_vars_into(a, &mut vars);
    collect_expr_vars_into(b, &mut vars);
    let mentions_own = own_var.is_some_and(|v| vars.contains(&v));
    let mentions_foreign = vars.iter().any(|v| Some(*v) != own_var);
    mentions_own && mentions_foreign
}

pub(super) fn collect_expr_vars_into<'e>(expr: &'e cfdb_core::query::Expr, acc: &mut Vec<&'e str>) {
    use cfdb_core::query::Expr as E;
    match expr {
        E::Property { var, .. } | E::Var(var) => acc.push(var.as_str()),
        E::Literal(_) | E::Param(_) => {}
        E::List(items) => {
            for item in items {
                collect_expr_vars_into(item, acc);
            }
        }
        E::Call { args, .. } => {
            for arg in args {
                collect_expr_vars_into(arg, acc);
            }
        }
    }
}

pub(super) fn matches_existing(existing: &Binding, h: NodeHandle) -> bool {
    matches!(existing, Binding::NodeRef(i) if *i == h)
}

pub(super) fn edge_label_matches(pattern: &EdgePattern, edge: &cfdb_core::fact::Edge) -> bool {
    match &pattern.label {
        Some(lbl) => edge.label == *lbl,
        None => true,
    }
}

pub(super) fn collect_pattern_vars(pattern: &Pattern) -> Vec<String> {
    let mut out = Vec::new();
    match pattern {
        Pattern::Node(np) => {
            if let Some(v) = &np.var {
                out.push(v.clone());
            }
        }
        Pattern::Path(pp) => {
            if let Some(v) = &pp.from.var {
                out.push(v.clone());
            }
            if let Some(v) = &pp.to.var {
                out.push(v.clone());
            }
            if let Some(v) = &pp.edge.var {
                out.push(v.clone());
            }
        }
        Pattern::Optional(inner) => out.extend(collect_pattern_vars(inner)),
        Pattern::Unwind { var, .. } => out.push(var.clone()),
    }
    out
}

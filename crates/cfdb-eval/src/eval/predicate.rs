use cfdb_core::fact::PropValue;
use cfdb_core::graph::GraphReader;
use cfdb_core::query::{CompareOp, Expr, ParamBinding, Predicate};
use cfdb_core::result::RowValue;

use super::{Binding, Bindings, Evaluator};

impl<'a, G: GraphReader + ?Sized> Evaluator<'a, G> {
    pub(super) fn eval_predicate(&self, predicate: &Predicate, bindings: &Bindings) -> bool {
        match predicate {
            Predicate::Compare { left, op, right } => {
                let lv = self.eval_expr(left, bindings);
                let rv = self.eval_expr(right, bindings);
                compare_propvalues(*op, lv.as_ref(), rv.as_ref())
            }
            Predicate::Ne { left, right } => {
                let lv = self.eval_expr(left, bindings);
                let rv = self.eval_expr(right, bindings);
                compare_propvalues(CompareOp::Ne, lv.as_ref(), rv.as_ref())
            }
            Predicate::In { left, right } => {
                let lv = self.eval_expr(left, bindings);
                let list = self.eval_expr_list(right, bindings);
                match (lv, list) {
                    (Some(v), Some(items)) => items.iter().any(|item| item == &v),
                    _ => false,
                }
            }
            Predicate::Regex { left, pattern } => {
                let lv = self.eval_expr(left, bindings);
                let pat = self.eval_expr(pattern, bindings);
                match (lv, pat) {
                    (Some(PropValue::Str(s)), Some(PropValue::Str(p))) => self
                        .compiled_regex(&p, |re| re.is_match(&s))
                        .unwrap_or(false),
                    _ => false,
                }
            }
            Predicate::NotExists { inner } => {
                let sub = Evaluator::new(self.state, self.params).run(inner);
                sub.rows.is_empty()
            }
            Predicate::And(a, b) => {
                self.eval_predicate(a, bindings) && self.eval_predicate(b, bindings)
            }
            Predicate::Or(a, b) => {
                self.eval_predicate(a, bindings) || self.eval_predicate(b, bindings)
            }
            Predicate::Not(inner) => !self.eval_predicate(inner, bindings),
        }
    }

    pub(super) fn eval_expr(&self, expr: &Expr, bindings: &Bindings) -> Option<PropValue> {
        match expr {
            Expr::Property { var, prop } => {
                let binding = bindings.get(var)?;
                match binding {
                    Binding::NodeRef(h) => self.state.node(*h)?.props.get(prop).cloned(),
                    Binding::EdgeRef(h) => {
                        let edge = self.state.edge(*h)?;
                        match prop.as_str() {
                            "label" => Some(PropValue::Str(edge.label.as_str().to_string())),
                            "src" => Some(PropValue::Str(edge.src.clone())),
                            "dst" => Some(PropValue::Str(edge.dst.clone())),
                            _ => edge.props.get(prop).cloned(),
                        }
                    }
                    Binding::Value(RowValue::Scalar(p)) => {
                        if prop.is_empty() {
                            Some(p.clone())
                        } else {
                            None
                        }
                    }
                    _ => None,
                }
            }
            Expr::Var(name) => bindings.get(name).and_then(|b| match b {
                Binding::Value(RowValue::Scalar(p)) => Some(p.clone()),
                Binding::NodeRef(h) => self
                    .state
                    .node(*h)
                    .map(|node| PropValue::Str(node.id.clone())),
                Binding::EdgeRef(h) => self
                    .state
                    .edge(*h)
                    .map(|edge| PropValue::Str(edge.label.as_str().to_string())),
                _ => None,
            }),
            Expr::Literal(p) => Some(p.clone()),
            Expr::Param(name) => match self.params.get(name) {
                Some(ParamBinding::Scalar(p)) => Some(p.clone()),
                _ => None,
            },
            Expr::List(_) => None,
            Expr::Call { name, args } => self.eval_call(name, args, bindings),
        }
    }

    pub(super) fn eval_expr_list(
        &self,
        expr: &Expr,
        bindings: &Bindings,
    ) -> Option<Vec<PropValue>> {
        match expr {
            Expr::List(items) => Some(
                items
                    .iter()
                    .filter_map(|e| self.eval_expr(e, bindings))
                    .collect(),
            ),
            Expr::Param(name) => match self.params.get(name) {
                Some(ParamBinding::List(items)) => Some(items.clone()),
                Some(ParamBinding::Scalar(p)) => Some(vec![p.clone()]),
                None => None,
            },
            other => self.eval_expr(other, bindings).map(|p| vec![p]),
        }
    }

    fn eval_call(&self, name: &str, args: &[Expr], bindings: &Bindings) -> Option<PropValue> {
        match name {
            "regexp_extract" => self.call_regexp_extract(args, bindings),
            "size" => self.call_size(args, bindings),
            "starts_with" => self.call_starts_with(args, bindings),
            "ends_with" => self.call_ends_with(args, bindings),
            "last_segment" => self.call_last_segment(args, bindings),
            "signature_divergent" => self.call_signature_divergent(args, bindings),
            "entries_subset" => self.call_entries_subset(args, bindings),
            "entries_jaccard" => self.call_entries_jaccard(args, bindings),
            "overlap_verdict" => self.call_overlap_verdict(args, bindings),
            _ => None,
        }
    }

    fn call_regexp_extract(&self, args: &[Expr], bindings: &Bindings) -> Option<PropValue> {
        let s = self.eval_expr(args.first()?, bindings)?;
        let pat = self.eval_expr(args.get(1)?, bindings)?;
        let (PropValue::Str(text), PropValue::Str(pattern)) = (s, pat) else {
            return None;
        };
        self.compiled_regex(&pattern, |re| {
            re.find(&text)
                .map(|m| PropValue::Str(m.as_str().to_string()))
        })
        .flatten()
    }

    fn call_size(&self, args: &[Expr], bindings: &Bindings) -> Option<PropValue> {
        let arg = args.first()?;
        if let Expr::Var(name) = arg {
            if let Some(Binding::Value(RowValue::List(items))) = bindings.get(name) {
                return Some(PropValue::Int(items.len() as i64));
            }
        }
        let PropValue::Str(s) = self.eval_expr(arg, bindings)? else {
            return None;
        };
        Some(PropValue::Int(s.chars().count() as i64))
    }

    fn call_starts_with(&self, args: &[Expr], bindings: &Bindings) -> Option<PropValue> {
        let s = self.eval_expr(args.first()?, bindings)?;
        let prefix = self.eval_expr(args.get(1)?, bindings)?;
        let (PropValue::Str(text), PropValue::Str(p)) = (s, prefix) else {
            return None;
        };
        Some(PropValue::Bool(text.starts_with(&p)))
    }

    fn call_ends_with(&self, args: &[Expr], bindings: &Bindings) -> Option<PropValue> {
        let s = self.eval_expr(args.first()?, bindings)?;
        let suffix = self.eval_expr(args.get(1)?, bindings)?;
        let (PropValue::Str(text), PropValue::Str(p)) = (s, suffix) else {
            return None;
        };
        Some(PropValue::Bool(text.ends_with(&p)))
    }

    fn call_last_segment(&self, args: &[Expr], bindings: &Bindings) -> Option<PropValue> {
        let PropValue::Str(text) = self.eval_expr(args.first()?, bindings)? else {
            return None;
        };
        Some(PropValue::Str(
            cfdb_core::qname::last_segment(&text).to_string(),
        ))
    }

    fn call_signature_divergent(&self, args: &[Expr], bindings: &Bindings) -> Option<PropValue> {
        let a = self.eval_expr(args.first()?, bindings)?;
        let b = self.eval_expr(args.get(1)?, bindings)?;
        let (PropValue::Str(sa), PropValue::Str(sb)) = (a, b) else {
            return None;
        };
        Some(PropValue::Bool(signatures_differ_modulo_whitespace(
            &sa, &sb,
        )))
    }

    fn call_entries_subset(&self, args: &[Expr], bindings: &Bindings) -> Option<PropValue> {
        let a = self.eval_expr(args.first()?, bindings)?;
        let b = self.eval_expr(args.get(1)?, bindings)?;
        let (PropValue::Str(sa), PropValue::Str(sb)) = (a, b) else {
            return None;
        };
        Some(PropValue::Bool(entries_subset_impl(&sa, &sb)))
    }

    fn call_entries_jaccard(&self, args: &[Expr], bindings: &Bindings) -> Option<PropValue> {
        let a = self.eval_expr(args.first()?, bindings)?;
        let b = self.eval_expr(args.get(1)?, bindings)?;
        let (PropValue::Str(sa), PropValue::Str(sb)) = (a, b) else {
            return None;
        };
        Some(PropValue::Float(entries_jaccard_impl(&sa, &sb)))
    }

    fn call_overlap_verdict(&self, args: &[Expr], bindings: &Bindings) -> Option<PropValue> {
        let a_norm = self.eval_expr(args.first()?, bindings)?;
        let b_norm = self.eval_expr(args.get(1)?, bindings)?;
        let a_hash = self.eval_expr(args.get(2)?, bindings)?;
        let b_hash = self.eval_expr(args.get(3)?, bindings)?;
        let (
            PropValue::Str(a_norm),
            PropValue::Str(b_norm),
            PropValue::Str(a_hash),
            PropValue::Str(b_hash),
        ) = (a_norm, b_norm, a_hash, b_hash)
        else {
            return None;
        };
        Some(PropValue::Str(
            overlap_verdict_impl(&a_norm, &b_norm, &a_hash, &b_hash).to_string(),
        ))
    }
}

mod udf;

use udf::{
    entries_jaccard_impl, entries_subset_impl, overlap_verdict_impl,
    signatures_differ_modulo_whitespace,
};

pub(super) fn compare_propvalues(
    op: CompareOp,
    a: Option<&PropValue>,
    b: Option<&PropValue>,
) -> bool {
    let (a, b) = match (a, b) {
        (Some(a), Some(b)) => (a, b),
        _ => return false,
    };
    let ord = match (a, b) {
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
        (PropValue::Null, PropValue::Null) => std::cmp::Ordering::Equal,
        _ => return false,
    };
    match op {
        CompareOp::Eq => ord == std::cmp::Ordering::Equal,
        CompareOp::Ne => ord != std::cmp::Ordering::Equal,
        CompareOp::Lt => ord == std::cmp::Ordering::Less,
        CompareOp::Le => ord != std::cmp::Ordering::Greater,
        CompareOp::Gt => ord == std::cmp::Ordering::Greater,
        CompareOp::Ge => ord != std::cmp::Ordering::Less,
    }
}

#[cfg(test)]
mod tests;

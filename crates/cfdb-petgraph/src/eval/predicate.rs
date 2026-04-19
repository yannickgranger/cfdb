//! Predicate and expression evaluation.
//!
//! `WHERE` predicates compose `Compare` / `In` / `Regex` / `NotExists` /
//! `And` / `Or` / `Not`. Expressions evaluate to `Option<PropValue>` so that
//! missing / `Null` bindings propagate cleanly into comparisons.

use cfdb_core::fact::PropValue;
use cfdb_core::query::{CompareOp, Expr, Param, Predicate};
use cfdb_core::result::RowValue;
use regex::Regex;

use super::{Binding, Bindings, Evaluator};

impl<'a> Evaluator<'a> {
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
                    (Some(PropValue::Str(s)), Some(PropValue::Str(p))) => {
                        Regex::new(&p).map(|re| re.is_match(&s)).unwrap_or(false)
                    }
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
                    Binding::NodeRef(idx) => self.state.graph[*idx].props.get(prop).cloned(),
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
                Binding::NodeRef(idx) => Some(PropValue::Str(self.state.graph[*idx].id.clone())),
                _ => None,
            }),
            Expr::Literal(p) => Some(p.clone()),
            Expr::Param(name) => match self.params.get(name) {
                Some(Param::Scalar(p)) => Some(p.clone()),
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
                Some(Param::List(items)) => Some(items.clone()),
                Some(Param::Scalar(p)) => Some(vec![p.clone()]),
                None => None,
            },
            other => self.eval_expr(other, bindings).map(|p| vec![p]),
        }
    }

    fn eval_call(&self, name: &str, args: &[Expr], bindings: &Bindings) -> Option<PropValue> {
        match name {
            "regexp_extract" => {
                let s = self.eval_expr(args.first()?, bindings)?;
                let pat = self.eval_expr(args.get(1)?, bindings)?;
                match (s, pat) {
                    (PropValue::Str(text), PropValue::Str(pattern)) => {
                        Regex::new(&pattern).ok().and_then(|re| {
                            re.find(&text)
                                .map(|m| PropValue::Str(m.as_str().to_string()))
                        })
                    }
                    _ => None,
                }
            }
            "size" => {
                let v = self.eval_expr(args.first()?, bindings)?;
                match v {
                    PropValue::Str(s) => Some(PropValue::Int(s.chars().count() as i64)),
                    _ => None,
                }
            }
            "starts_with" => {
                let s = self.eval_expr(args.first()?, bindings)?;
                let prefix = self.eval_expr(args.get(1)?, bindings)?;
                match (s, prefix) {
                    (PropValue::Str(text), PropValue::Str(p)) => {
                        Some(PropValue::Bool(text.starts_with(&p)))
                    }
                    _ => None,
                }
            }
            "ends_with" => {
                let s = self.eval_expr(args.first()?, bindings)?;
                let suffix = self.eval_expr(args.get(1)?, bindings)?;
                match (s, suffix) {
                    (PropValue::Str(text), PropValue::Str(p)) => {
                        Some(PropValue::Bool(text.ends_with(&p)))
                    }
                    _ => None,
                }
            }
            "last_segment" => {
                let s = self.eval_expr(args.first()?, bindings)?;
                match s {
                    PropValue::Str(text) => {
                        let seg = match text.rfind(':') {
                            Some(i) => text[i + 1..].to_string(),
                            None => text,
                        };
                        Some(PropValue::Str(seg))
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }
}

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

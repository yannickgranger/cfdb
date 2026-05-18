//! Predicate and expression evaluation.
//!
//! `WHERE` predicates compose `Compare` / `In` / `Regex` / `NotExists` /
//! `And` / `Or` / `Not`. Expressions evaluate to `Option<PropValue>` so that
//! missing / `Null` bindings propagate cleanly into comparisons.

use cfdb_core::fact::PropValue;
use cfdb_core::query::{CompareOp, Expr, Param, Predicate};
use cfdb_core::result::RowValue;

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
                    Binding::NodeRef(idx) => self.state.graph[*idx].props.get(prop).cloned(),
                    Binding::EdgeRef(idx) => {
                        let edge = self.state.graph.edge_weight(*idx)?;
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
                Binding::NodeRef(idx) => Some(PropValue::Str(self.state.graph[*idx].id.clone())),
                Binding::EdgeRef(idx) => self
                    .state
                    .graph
                    .edge_weight(*idx)
                    .map(|edge| PropValue::Str(edge.label.as_str().to_string())),
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
        // `size(var)` where `var` was bound by a prior WITH `collect(...)`
        // aggregation resolves to a `RowValue::List` binding — lift the
        // list's length into an Int directly. `eval_expr` cannot surface
        // List values (it is scalar-typed), so this path has to read the
        // binding itself. RFC-036 §3.2 / issue #202: the VSB detector's
        // `WHERE size(resolvers) > 1` clause depends on this code path.
        if let Expr::Var(name) = arg {
            if let Some(Binding::Value(RowValue::List(items))) = bindings.get(name) {
                return Some(PropValue::Int(items.len() as i64));
            }
        }
        // Fallback — `size(str)` counts Unicode scalar values (chars).
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

    /// `last_segment(text) -> Str` Cypher UDF — delegates to
    /// [`cfdb_core::qname::last_segment`], the RFC-035 §3.3
    /// invariant-owner of the qname `last_segment` formula.
    ///
    /// Every consumer of the formula in the workspace routes through
    /// the canonical owner: the index-build write side via
    /// [`crate::index::ComputedKey::evaluate`] (slice 3), and this
    /// query-time read side (slice 4). The
    /// `call_last_segment_agrees_with_canonical_owner_byte_for_byte`
    /// test in this module's `#[cfg(test)]` block pins the dispatch
    /// against the canonical helper on the canary set used by the
    /// slice 3 self-dogfood.
    ///
    /// Returns `None` on non-string inputs (preserves the
    /// `?`-on-type-mismatch surface shared with the other UDFs in
    /// this dispatcher).
    fn call_last_segment(&self, args: &[Expr], bindings: &Bindings) -> Option<PropValue> {
        let PropValue::Str(text) = self.eval_expr(args.first()?, bindings)? else {
            return None;
        };
        Some(PropValue::Str(
            cfdb_core::qname::last_segment(&text).to_string(),
        ))
    }

    /// `signature_divergent(sig_a, sig_b) -> Bool` — issue #47.
    ///
    /// Returns `true` when two `:Item.signature` strings (produced by
    /// `cfdb-extractor::type_render::render_fn_signature`) differ after
    /// whitespace normalization. This is the load-bearing discriminator
    /// for the DDD Shared-Kernel-vs-Homonym check (RFC-029 §A1.5 gate
    /// v0.2-8, `council/RATIFIED.md` R1): two items that share a last
    /// qname segment across bounded contexts are a Shared Kernel when
    /// their signatures match byte-for-byte and a Context Homonym when
    /// they diverge.
    ///
    /// # Normalization
    ///
    /// Both inputs are trimmed and internal whitespace is collapsed to
    /// single spaces before comparison. This keeps the UDF robust
    /// against harmless whitespace noise in the extractor's future
    /// evolution. Parameter NAMES are already normalized out by
    /// `render_fn_signature` at extract time (it emits types only), so
    /// this UDF does not re-enact that normalization.
    ///
    /// # Type-mismatch behavior
    ///
    /// Matches the convention of the other hard-wired UDFs
    /// (`starts_with`, `ends_with`): if either argument is not a
    /// string, the UDF returns `None`, which the predicate evaluator
    /// treats as "unknown" — the enclosing `WHERE` clause rejects the
    /// binding rather than silently coercing to `true` or `false`. This
    /// is the correct failure mode for the classifier (#48) — an item
    /// with no `:Item.signature` prop (non-fn kinds) should not surface
    /// as a divergent pair simply because the prop is absent.
    fn call_signature_divergent(&self, args: &[Expr], bindings: &Bindings) -> Option<PropValue> {
        let a = self.eval_expr(args.first()?, bindings)?;
        let b = self.eval_expr(args.get(1)?, bindings)?;
        let (PropValue::Str(sa), PropValue::Str(sb)) = (a, b) else {
            return None;
        };
        // #409 perf — was `normalize_signature(&sa) != normalize_signature(&sb)`,
        // which allocated TWO new Strings per call. Hot path on
        // signature-divergent.cypher: ~5M invocations per smoke run on
        // cfdb-self drove the query to 542s wall-time in CI run 575.
        // `signatures_differ_modulo_whitespace` walks both inputs once
        // with no allocation, skipping whitespace runs as a single
        // logical space — equivalent semantics, ~order-of-magnitude
        // less constant-factor cost.
        Some(PropValue::Bool(signatures_differ_modulo_whitespace(
            &sa, &sb,
        )))
    }

    /// `entries_subset(a, b) -> Bool` — RFC-040 §3.4.
    ///
    /// Returns `true` iff every element of JSON-array `a` is contained
    /// in JSON-array `b`. The empty set is a subset of anything; equal
    /// sets are subsets of each other. Operates on the
    /// `:ConstTable.entries_normalized` wire shape (RFC-040 §3.4): a
    /// JSON array of strings (e.g. `["EUR","USD"]`) or numbers
    /// (e.g. `[1,42,100]`). Element type is inferred from the first
    /// parsable element; mixed-element-type inputs return `false`
    /// (treated as no overlap — RFC-040 §3.4 N2).
    ///
    /// # Type-mismatch behavior
    ///
    /// Non-string args (or non-JSON-array strings) return `None`,
    /// matching the convention of the other hard-wired UDFs.
    fn call_entries_subset(&self, args: &[Expr], bindings: &Bindings) -> Option<PropValue> {
        let a = self.eval_expr(args.first()?, bindings)?;
        let b = self.eval_expr(args.get(1)?, bindings)?;
        let (PropValue::Str(sa), PropValue::Str(sb)) = (a, b) else {
            return None;
        };
        Some(PropValue::Bool(entries_subset_impl(&sa, &sb)))
    }

    /// `entries_jaccard(a, b) -> Float` — RFC-040 §3.4.
    ///
    /// Returns `|a ∩ b| / |a ∪ b|`. Returns `0.0` when both inputs
    /// are empty (avoid divide-by-zero). Operates on the
    /// `:ConstTable.entries_normalized` wire shape (RFC-040 §3.4).
    /// Mixed-element-type inputs return `0.0` (treated as no overlap
    /// — RFC-040 §3.4 N2).
    ///
    /// # Type-mismatch behavior
    ///
    /// Non-string args (or non-JSON-array strings) return `None`.
    fn call_entries_jaccard(&self, args: &[Expr], bindings: &Bindings) -> Option<PropValue> {
        let a = self.eval_expr(args.first()?, bindings)?;
        let b = self.eval_expr(args.get(1)?, bindings)?;
        let (PropValue::Str(sa), PropValue::Str(sb)) = (a, b) else {
            return None;
        };
        Some(PropValue::Float(entries_jaccard_impl(&sa, &sb)))
    }

    /// `overlap_verdict(a_normalized, b_normalized, a_hash, b_hash) -> Str`
    /// — RFC-040 §3.4 precedence-decoder.
    ///
    /// Maps a `(a, b)` pair to one of `'CONST_TABLE_DUPLICATE'`,
    /// `'CONST_TABLE_SUBSET'`, `'CONST_TABLE_INTERSECTION_HIGH'`, or
    /// `'CONST_TABLE_NONE'` per the precedence ordering in RFC-040 §3.4:
    ///
    ///   1. DUPLICATE: `a_hash = b_hash` (entries_hash equality is the
    ///      canonical set-equality key, RFC-040 §3.1).
    ///   2. SUBSET: not duplicate AND `entries_subset(a, b)` OR
    ///      `entries_subset(b, a)`.
    ///   3. INTERSECTION_HIGH: not subset AND
    ///      `entries_jaccard(a, b) >= 0.5`.
    ///   4. otherwise: `'CONST_TABLE_NONE'`.
    ///
    /// Lives here (alongside `entries_subset` / `entries_jaccard`)
    /// because the v0.1 Cypher subset has no `CASE WHEN` / `UNION`
    /// (`crates/cfdb-query/src/parser/mod.rs` §316–432), so the
    /// precedence-decoder MUST live in a UDF for the rule file to
    /// emit a single `verdict` string column. The precedence semantics
    /// are RFC-040-load-bearing — keeping them in one Rust function
    /// (rather than reimplemented in every consumer query) is the
    /// canonical-resolver pattern (RFC-035 §3.3).
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

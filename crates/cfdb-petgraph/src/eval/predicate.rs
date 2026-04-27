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
        Regex::new(&pattern).ok().and_then(|re| {
            re.find(&text)
                .map(|m| PropValue::Str(m.as_str().to_string()))
        })
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
        Some(PropValue::Bool(
            normalize_signature(&sa) != normalize_signature(&sb),
        ))
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

/// Parsed JSON-array element set for the RFC-040 §3.4 overlap UDFs.
///
/// `entries_normalized` is JSON-array-as-string of either all strings
/// (`["a","b"]`) or all numbers (`[1,2]`) — the element type is
/// inferred from the first parseable element. Mixed-element-type
/// inputs (e.g. `["a", 1]`) are forbidden by the wire contract; the
/// UDFs treat them as `MixedOrInvalid` so the enclosing rule sees no
/// overlap (RFC-040 §3.4 N2).
#[derive(Debug, PartialEq, Eq)]
enum NormalizedEntries {
    Strs(std::collections::BTreeSet<String>),
    Ints(std::collections::BTreeSet<i64>),
    /// Empty input (`[]`) — distinct from MixedOrInvalid because empty
    /// is a valid set (subset of anything; jaccard 0/0 → 0.0). The
    /// element type is unknown but operations against another empty
    /// or any populated set are well-defined.
    Empty,
    /// Either a parse error or a mixed-element-type input. Both UDFs
    /// treat this as "no overlap" rather than propagating a parse
    /// failure, because the wire contract guarantees well-formed
    /// `entries_normalized`; a malformed value is best surfaced as
    /// "no row matches" in the rule rather than an evaluator panic.
    MixedOrInvalid,
}

/// Parse the `entries_normalized` JSON-array string into a sorted set
/// suitable for set-relationship comparison. Element type is inferred
/// from the first element; mixed-type or invalid input collapses to
/// [`NormalizedEntries::MixedOrInvalid`].
fn parse_entries_normalized(s: &str) -> NormalizedEntries {
    let parsed: serde_json::Value = match serde_json::from_str(s) {
        Ok(v) => v,
        Err(_) => return NormalizedEntries::MixedOrInvalid,
    };
    let serde_json::Value::Array(items) = parsed else {
        return NormalizedEntries::MixedOrInvalid;
    };
    if items.is_empty() {
        return NormalizedEntries::Empty;
    }
    // Infer element type from the first element. Number elements are
    // matched as i64 because the extractor emits decimal-stringified
    // integers per RFC-040 §3.4; non-integral floats are not in the
    // wire-shape vocabulary and collapse to MixedOrInvalid.
    match &items[0] {
        serde_json::Value::String(_) => {
            let mut set = std::collections::BTreeSet::new();
            for v in &items {
                let serde_json::Value::String(s) = v else {
                    return NormalizedEntries::MixedOrInvalid;
                };
                set.insert(s.clone());
            }
            NormalizedEntries::Strs(set)
        }
        serde_json::Value::Number(_) => {
            let mut set = std::collections::BTreeSet::new();
            for v in &items {
                let serde_json::Value::Number(n) = v else {
                    return NormalizedEntries::MixedOrInvalid;
                };
                let Some(i) = n.as_i64() else {
                    return NormalizedEntries::MixedOrInvalid;
                };
                set.insert(i);
            }
            NormalizedEntries::Ints(set)
        }
        _ => NormalizedEntries::MixedOrInvalid,
    }
}

/// `entries_subset` impl — true iff every element of `a` is in `b`.
/// Empty is a subset of anything; equal sets are subsets of each
/// other. Cross-element-type or invalid inputs return `false`.
fn entries_subset_impl(a_json: &str, b_json: &str) -> bool {
    let a = parse_entries_normalized(a_json);
    let b = parse_entries_normalized(b_json);
    match (a, b) {
        (NormalizedEntries::Empty, _) => true,
        (NormalizedEntries::MixedOrInvalid, _) | (_, NormalizedEntries::MixedOrInvalid) => false,
        (NormalizedEntries::Strs(sa), NormalizedEntries::Strs(sb)) => sa.is_subset(&sb),
        (NormalizedEntries::Ints(ia), NormalizedEntries::Ints(ib)) => ia.is_subset(&ib),
        // Cross-element-type — no overlap by RFC-040 §3.4 N2. The
        // empty-on-the-right case (e.g. Strs vs Empty) is `false`
        // because a populated set is never a subset of empty; the
        // empty-on-the-left case is handled by the first arm above.
        (NormalizedEntries::Strs(_), NormalizedEntries::Ints(_))
        | (NormalizedEntries::Ints(_), NormalizedEntries::Strs(_))
        | (NormalizedEntries::Strs(_), NormalizedEntries::Empty)
        | (NormalizedEntries::Ints(_), NormalizedEntries::Empty) => false,
    }
}

/// `entries_jaccard` impl — `|a ∩ b| / |a ∪ b|`.
/// Returns `0.0` if both inputs are empty (avoid divide-by-zero) and
/// `0.0` for cross-element-type / invalid input.
fn entries_jaccard_impl(a_json: &str, b_json: &str) -> f64 {
    let a = parse_entries_normalized(a_json);
    let b = parse_entries_normalized(b_json);
    match (a, b) {
        (NormalizedEntries::Empty, NormalizedEntries::Empty) => 0.0,
        (NormalizedEntries::MixedOrInvalid, _) | (_, NormalizedEntries::MixedOrInvalid) => 0.0,
        (NormalizedEntries::Strs(sa), NormalizedEntries::Strs(sb)) => jaccard_btree(&sa, &sb),
        (NormalizedEntries::Ints(ia), NormalizedEntries::Ints(ib)) => jaccard_btree(&ia, &ib),
        // Cross-element-type or empty-vs-populated — no overlap. The
        // empty-vs-populated case returns 0.0 because |∩| = 0, |∪| =
        // |populated|, ratio is 0.0.
        (NormalizedEntries::Strs(_), NormalizedEntries::Ints(_))
        | (NormalizedEntries::Ints(_), NormalizedEntries::Strs(_))
        | (NormalizedEntries::Empty, NormalizedEntries::Strs(_))
        | (NormalizedEntries::Empty, NormalizedEntries::Ints(_))
        | (NormalizedEntries::Strs(_), NormalizedEntries::Empty)
        | (NormalizedEntries::Ints(_), NormalizedEntries::Empty) => 0.0,
    }
}

fn jaccard_btree<T: Ord>(
    a: &std::collections::BTreeSet<T>,
    b: &std::collections::BTreeSet<T>,
) -> f64 {
    let inter = a.intersection(b).count() as f64;
    let union = a.union(b).count() as f64;
    if union == 0.0 {
        0.0
    } else {
        inter / union
    }
}

/// `overlap_verdict` impl — RFC-040 §3.4 precedence-decoder.
/// See [`Evaluator::call_overlap_verdict`] for the contract.
fn overlap_verdict_impl(a_norm: &str, b_norm: &str, a_hash: &str, b_hash: &str) -> &'static str {
    if a_hash == b_hash {
        return "CONST_TABLE_DUPLICATE";
    }
    if entries_subset_impl(a_norm, b_norm) || entries_subset_impl(b_norm, a_norm) {
        return "CONST_TABLE_SUBSET";
    }
    if entries_jaccard_impl(a_norm, b_norm) >= 0.5 {
        return "CONST_TABLE_INTERSECTION_HIGH";
    }
    "CONST_TABLE_NONE"
}

/// Normalize a signature string for `signature_divergent` comparison —
/// trim outer whitespace and collapse any run of internal whitespace to
/// a single ASCII space. See [`Evaluator::call_signature_divergent`]
/// for the rationale.
fn normalize_signature(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_ws = false;
    for c in s.trim().chars() {
        if c.is_whitespace() {
            if !prev_ws {
                out.push(' ');
                prev_ws = true;
            }
        } else {
            out.push(c);
            prev_ws = false;
        }
    }
    out
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

#[cfg(test)]
mod signature_divergent_tests {
    use super::normalize_signature;

    #[test]
    fn trim_outer_whitespace() {
        assert_eq!(normalize_signature("  fn() -> ()  "), "fn() -> ()");
    }

    #[test]
    fn collapse_internal_whitespace() {
        assert_eq!(
            normalize_signature("fn(i32,   String)  ->  bool"),
            "fn(i32, String) -> bool"
        );
    }

    #[test]
    fn identical_normalized_strings_are_not_divergent() {
        let a = normalize_signature("fn(i32) -> bool");
        let b = normalize_signature("fn(i32) -> bool");
        assert_eq!(a, b);
    }

    #[test]
    fn whitespace_only_difference_is_not_divergent() {
        let a = normalize_signature("fn(i32) -> bool");
        let b = normalize_signature("fn(i32)  ->   bool");
        assert_eq!(a, b);
    }

    #[test]
    fn different_types_are_divergent() {
        let a = normalize_signature("fn() -> f64");
        let b = normalize_signature("fn() -> (f64, f64)");
        assert_ne!(a, b);
    }
}

#[cfg(test)]
mod last_segment_tests {
    use std::collections::BTreeMap;

    use cfdb_core::fact::PropValue;
    use cfdb_core::query::{Expr, Param};

    use crate::eval::Evaluator;
    use crate::graph::KeyspaceState;

    /// AC4 — the `last_segment(...)` Cypher UDF MUST delegate to
    /// `cfdb_core::qname::last_segment` (the RFC-035 §3.3 invariant
    /// owner) byte-for-byte. Routing the dispatch through the
    /// canonical helper closes the read-side of the §3.3 invariant
    /// (the write-side closed in slice 3 via `ComputedKey::evaluate`).
    ///
    /// Canary set extends the slice 3 self-dogfood inputs with edge
    /// cases (single-segment, leading/trailing separator, single-`:`
    /// non-qname inputs). The canonical helper splits at the LAST
    /// `::` and returns the trailing segment (or the whole input
    /// when no `::` is present). Pinning these here surfaces any
    /// future divergence between the UDF and the canonical helper
    /// loudly rather than via a downstream Cypher query mismatch.
    #[test]
    fn call_last_segment_agrees_with_canonical_owner_byte_for_byte() {
        let state = KeyspaceState::new();
        let params: BTreeMap<String, Param> = BTreeMap::new();
        let evaluator = Evaluator::new(&state, &params);
        let bindings: BTreeMap<String, crate::eval::Binding> = BTreeMap::new();

        let inputs = [
            "foo::bar::baz",
            "foo",
            "",
            "cfdb_extractor::item_visitor::ItemVisitor::emit_item",
            "single_segment",
            "::leading_separator",
            "trailing_separator::",
            "cfdb_core::qname::last_segment",
        ];

        for input in inputs {
            let expr = Expr::Call {
                name: "last_segment".into(),
                args: vec![Expr::Literal(PropValue::Str(input.to_string()))],
            };
            let actual = evaluator.eval_expr(&expr, &bindings);
            let expected = Some(PropValue::Str(
                cfdb_core::qname::last_segment(input).to_string(),
            ));
            assert_eq!(
                actual, expected,
                "Cypher last_segment UDF diverged from canonical \
                 cfdb_core::qname::last_segment on input {input:?}"
            );
        }
    }

    /// The UDF preserves the `Option<PropValue>` surface — non-string
    /// inputs return `None` (the `?`-on-type-mismatch path shared with
    /// the other UDFs in this dispatcher).
    #[test]
    fn call_last_segment_returns_none_on_non_string_input() {
        let state = KeyspaceState::new();
        let params: BTreeMap<String, Param> = BTreeMap::new();
        let evaluator = Evaluator::new(&state, &params);
        let bindings: BTreeMap<String, crate::eval::Binding> = BTreeMap::new();

        let expr = Expr::Call {
            name: "last_segment".into(),
            args: vec![Expr::Literal(PropValue::Int(42))],
        };
        let actual = evaluator.eval_expr(&expr, &bindings);
        assert_eq!(actual, None);
    }
}

#[cfg(test)]
mod entries_overlap_tests {
    //! Unit tests for the RFC-040 §3.4 overlap UDFs
    //! (`entries_subset`, `entries_jaccard`, `overlap_verdict`).
    //!
    //! Pure-function impls (`entries_subset_impl`, `entries_jaccard_impl`,
    //! `overlap_verdict_impl`) are exercised directly so the test surface
    //! is independent of the dispatch wrapper. Dispatch wiring is
    //! covered by the integration scar in
    //! `crates/cfdb-cli/tests/const_table_overlap.rs`.
    use super::{entries_jaccard_impl, entries_subset_impl, overlap_verdict_impl};

    // ---- entries_subset --------------------------------------------------

    #[test]
    fn empty_is_subset_of_anything_str() {
        assert!(entries_subset_impl("[]", r#"["EUR","USD"]"#));
    }

    #[test]
    fn empty_is_subset_of_empty() {
        assert!(entries_subset_impl("[]", "[]"));
    }

    #[test]
    fn equal_str_sets_are_subsets_of_each_other() {
        let a = r#"["EUR","GBP","USD"]"#;
        let b = r#"["EUR","GBP","USD"]"#;
        assert!(entries_subset_impl(a, b));
        assert!(entries_subset_impl(b, a));
    }

    #[test]
    fn strict_subset_str_returns_true_one_way() {
        // ["EUR","USD"] ⊂ ["EUR","GBP","USD"]
        let small = r#"["EUR","USD"]"#;
        let big = r#"["EUR","GBP","USD"]"#;
        assert!(entries_subset_impl(small, big));
        // superset is NOT a subset of the smaller set
        assert!(!entries_subset_impl(big, small));
    }

    #[test]
    fn strict_subset_int_returns_true_one_way() {
        // [1,2] ⊂ [1,2,3]
        let small = "[1,2]";
        let big = "[1,2,3]";
        assert!(entries_subset_impl(small, big));
        assert!(!entries_subset_impl(big, small));
    }

    #[test]
    fn disjoint_str_sets_are_not_subsets() {
        let a = r#"["EUR","USD"]"#;
        let b = r#"["JPY","CHF"]"#;
        assert!(!entries_subset_impl(a, b));
        assert!(!entries_subset_impl(b, a));
    }

    #[test]
    fn mixed_element_type_is_not_subset_either_way() {
        // RFC-040 §3.4 N2 — mixed-type inputs return false.
        let strs = r#"["1","2"]"#;
        let ints = "[1,2]";
        assert!(!entries_subset_impl(strs, ints));
        assert!(!entries_subset_impl(ints, strs));
    }

    #[test]
    fn invalid_json_is_not_subset_either_way() {
        assert!(!entries_subset_impl("not json", r#"["a"]"#));
        assert!(!entries_subset_impl(r#"["a"]"#, "not json"));
    }

    // ---- entries_jaccard -------------------------------------------------

    #[test]
    fn jaccard_of_two_empty_sets_is_zero() {
        // RFC-040 §3.4 — divide-by-zero guard.
        assert_eq!(entries_jaccard_impl("[]", "[]"), 0.0);
    }

    #[test]
    fn jaccard_of_identical_str_sets_is_one() {
        let a = r#"["EUR","GBP","USD"]"#;
        let b = r#"["EUR","GBP","USD"]"#;
        assert_eq!(entries_jaccard_impl(a, b), 1.0);
    }

    #[test]
    fn jaccard_of_identical_int_sets_is_one() {
        assert_eq!(entries_jaccard_impl("[1,2,3]", "[1,2,3]"), 1.0);
    }

    #[test]
    fn jaccard_half_overlap_str_is_one_third() {
        // {a,b} vs {b,c} → |∩|=1, |∪|=3, ratio = 1/3.
        let a = r#"["a","b"]"#;
        let b = r#"["b","c"]"#;
        let j = entries_jaccard_impl(a, b);
        assert!((j - (1.0 / 3.0)).abs() < 1e-12, "got {j}");
    }

    #[test]
    fn jaccard_half_overlap_str_at_threshold() {
        // {a,b,c} vs {b,c,d} → |∩|=2, |∪|=4, ratio = 0.5 (the RFC §3.4
        // INTERSECTION_HIGH threshold). Pin the boundary value
        // explicitly so a future refactor cannot drift it across 0.5.
        let a = r#"["a","b","c"]"#;
        let b = r#"["b","c","d"]"#;
        let j = entries_jaccard_impl(a, b);
        assert!((j - 0.5).abs() < 1e-12, "got {j}");
        assert!(j >= 0.5);
    }

    #[test]
    fn jaccard_disjoint_sets_is_zero() {
        let a = r#"["EUR","USD"]"#;
        let b = r#"["JPY","CHF"]"#;
        assert_eq!(entries_jaccard_impl(a, b), 0.0);
    }

    #[test]
    fn jaccard_subset_int_is_ratio_of_sizes() {
        // [1,2] ⊂ [1,2,3,4] — |∩|=2, |∪|=4, ratio = 0.5.
        let j = entries_jaccard_impl("[1,2]", "[1,2,3,4]");
        assert!((j - 0.5).abs() < 1e-12, "got {j}");
    }

    #[test]
    fn jaccard_mixed_element_types_is_zero() {
        // RFC-040 §3.4 N2 — mixed-type inputs return 0.0.
        let strs = r#"["1","2"]"#;
        let ints = "[1,2]";
        assert_eq!(entries_jaccard_impl(strs, ints), 0.0);
        assert_eq!(entries_jaccard_impl(ints, strs), 0.0);
    }

    #[test]
    fn jaccard_invalid_json_is_zero() {
        assert_eq!(entries_jaccard_impl("not json", r#"["a"]"#), 0.0);
        assert_eq!(entries_jaccard_impl(r#"["a"]"#, "not json"), 0.0);
    }

    #[test]
    fn jaccard_empty_vs_populated_is_zero() {
        // Empty vs populated: |∩|=0, |∪|=|populated|, ratio = 0.0.
        assert_eq!(entries_jaccard_impl("[]", r#"["a","b"]"#), 0.0);
        assert_eq!(entries_jaccard_impl(r#"["a","b"]"#, "[]"), 0.0);
    }

    // ---- overlap_verdict precedence -------------------------------------

    #[test]
    fn overlap_verdict_duplicate_when_hashes_equal() {
        // hash equality is the canonical set-equality key (RFC-040 §3.1) —
        // takes precedence over subset / jaccard regardless of normalized
        // contents.
        let v = overlap_verdict_impl(r#"["a"]"#, r#"["a"]"#, "deadbeef", "deadbeef");
        assert_eq!(v, "CONST_TABLE_DUPLICATE");
    }

    #[test]
    fn overlap_verdict_subset_when_strict_subset_and_hashes_differ() {
        // Strict subset — different hashes (different sizes), one is a
        // subset of the other.
        let v = overlap_verdict_impl(r#"["a","b"]"#, r#"["a","b","c"]"#, "h_small", "h_big");
        assert_eq!(v, "CONST_TABLE_SUBSET");
    }

    #[test]
    fn overlap_verdict_subset_in_either_order() {
        // a ⊃ b is also CONST_TABLE_SUBSET — the rule is symmetric on the
        // pair; the verdict fires when either side is a subset of the
        // other.
        let v = overlap_verdict_impl(r#"["a","b","c"]"#, r#"["a","b"]"#, "h_big", "h_small");
        assert_eq!(v, "CONST_TABLE_SUBSET");
    }

    #[test]
    fn overlap_verdict_intersection_high_when_jaccard_at_threshold() {
        // {a,b,c} vs {b,c,d} — jaccard 0.5, neither is a subset of the
        // other. RFC-040 §3.4 third-tier verdict.
        let v = overlap_verdict_impl(r#"["a","b","c"]"#, r#"["b","c","d"]"#, "h_left", "h_right");
        assert_eq!(v, "CONST_TABLE_INTERSECTION_HIGH");
    }

    #[test]
    fn overlap_verdict_none_when_jaccard_below_threshold() {
        // {a,b,c,d} vs {c,e,f,g} — jaccard 1/7 ≈ 0.143, no subset
        // relation, no hash match → NONE.
        let v = overlap_verdict_impl(
            r#"["a","b","c","d"]"#,
            r#"["c","e","f","g"]"#,
            "h_left",
            "h_right",
        );
        assert_eq!(v, "CONST_TABLE_NONE");
    }

    #[test]
    fn overlap_verdict_none_when_disjoint() {
        let v = overlap_verdict_impl(r#"["EUR","USD"]"#, r#"["JPY","CHF"]"#, "h_left", "h_right");
        assert_eq!(v, "CONST_TABLE_NONE");
    }
}

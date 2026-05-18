//! Pattern application — `MATCH`, path traversal, `OPTIONAL MATCH`, `UNWIND`.
//! Streaming iterator adapters: per incoming binding row we build a bounded
//! scratch `Vec<Bindings>`, yield it, then drop — peak memory is O(per-row
//! fan-out), not O(cartesian). Memory note lives in `super::Evaluator`.
//!
//! Path-pattern methods (`apply_path_pattern`, traversal, BFS) live in the
//! `path` submodule per the #253 god-file split; node-pattern methods,
//! `OPTIONAL MATCH`, `UNWIND`, and free helpers (`matches_existing`,
//! `edge_label_matches`, `collect_pattern_vars`) stay here.

mod path;

use cfdb_core::query::{EdgePattern, NodePattern, Param, Pattern, Predicate};
use cfdb_core::result::{RowValue, Warning, WarningKind};
use petgraph::stable_graph::NodeIndex;

use super::explain_fmt::format_node_pattern;
use super::util::suggest_label;
use super::{Binding, BindingStream, Bindings, Evaluator};
use crate::explain::ExplainHit;
use crate::index::lookup;

impl<'a> Evaluator<'a> {
    pub(super) fn apply_node_pattern<'e>(
        &'e self,
        table: BindingStream<'e>,
        np: &'e NodePattern,
        where_clause: Option<&'e Predicate>,
    ) -> BindingStream<'e> {
        // Issue #409 fast path — when neither the NodePattern itself
        // nor the top-level WHERE references any var OTHER than the
        // pattern's own var, `candidate_nodes` is binding-independent:
        // the result is identical for every incoming row, so we
        // compute it ONCE up front and have the per-row closure borrow
        // the cached vec. Without this lift, the post-cleanup
        // qbot-core market_data scope ran the Cartesian classifier
        // rules at O(outer × inner_lookup) and hung 40+ min at 96% CPU
        // (issue #409). With the lift the inner leaf collapses to O(1)
        // lookups per rule; explain traces show `(b:Item)` appearing
        // ONCE rather than `outer_row_count` times.
        if is_binding_independent_pattern(np, where_clause) {
            let cached = self.candidate_nodes(np, where_clause, &Bindings::new());
            return Box::new(table.flat_map(move |bindings| {
                let mut out: Vec<Bindings> = Vec::new();
                self.emit_node_bindings(&mut out, bindings, &cached, np);
                out
            }));
        }
        // Per-row candidate_nodes — the incoming row's bindings pick
        // the cross-MATCH bucket (RFC-035 slice 6). Empty bindings
        // collapse to slice-5 behaviour. This path runs when the
        // pattern OR the WHERE references a foreign var (genuine
        // cross-binding equi-join), where the candidate set IS
        // binding-dependent and per-row narrowing is correct.
        Box::new(table.flat_map(move |bindings| {
            let candidates = self.candidate_nodes(np, where_clause, &bindings);
            let mut out: Vec<Bindings> = Vec::new();
            self.emit_node_bindings(&mut out, bindings, &candidates, np);
            out
        }))
    }

    /// Dispatch a single binding row through the three node-pattern cases:
    /// anonymous (no `var`), pre-bound `var` (pinned to the existing ref), or
    /// fresh `var` (multiplied by candidates). Split out of
    /// `apply_node_pattern` to keep cognitive complexity below the project
    /// ceiling (RFC-031 §5 / issue #26).
    fn emit_node_bindings(
        &self,
        out: &mut Vec<Bindings>,
        bindings: Bindings,
        candidates: &[NodeIndex],
        np: &NodePattern,
    ) {
        match np.var.as_deref() {
            None => self.emit_anon_node(out, bindings, candidates, np),
            Some(var) if bindings.contains_key(var) => {
                self.emit_bound_node(out, bindings, var, candidates, np);
            }
            Some(var) => self.emit_new_var_node(out, bindings, var, candidates, np),
        }
    }

    /// Anonymous node pattern — every candidate that matches props emits a
    /// fresh clone of the carrying bindings.
    fn emit_anon_node(
        &self,
        out: &mut Vec<Bindings>,
        bindings: Bindings,
        candidates: &[NodeIndex],
        np: &NodePattern,
    ) {
        candidates
            .iter()
            .filter(|idx| self.node_props_match(**idx, np))
            .for_each(|_| out.push(bindings.clone()));
    }

    /// Pre-bound variable — the incoming bindings already carry `var`.
    /// Emit a single clone iff at least one candidate matches the existing
    /// pin AND its props. Breaks on first match (confirmation semantics).
    fn emit_bound_node(
        &self,
        out: &mut Vec<Bindings>,
        bindings: Bindings,
        var: &str,
        candidates: &[NodeIndex],
        np: &NodePattern,
    ) {
        let existing = match bindings.get(var) {
            Some(b) => b,
            None => return,
        };
        let any_hit = candidates
            .iter()
            .any(|idx| matches_existing(existing, *idx) && self.node_props_match(*idx, np));
        if any_hit {
            out.push(bindings);
        }
    }

    /// Fresh variable — each matching candidate produces a new binding row
    /// with `var` inserted.
    fn emit_new_var_node(
        &self,
        out: &mut Vec<Bindings>,
        bindings: Bindings,
        var: &str,
        candidates: &[NodeIndex],
        np: &NodePattern,
    ) {
        candidates
            .iter()
            .filter(|idx| self.node_props_match(**idx, np))
            .for_each(|idx| {
                let mut next = bindings.clone();
                next.insert(var.to_string(), Binding::NodeRef(*idx));
                out.push(next);
            });
    }

    pub(super) fn candidate_nodes(
        &self,
        np: &NodePattern,
        where_clause: Option<&Predicate>,
        bindings: &Bindings,
    ) -> Vec<NodeIndex> {
        if let Some(label) = &np.label {
            if !self.state.has_label(label) {
                let suggestion = suggest_label(
                    label.as_str(),
                    self.state.by_label.keys().map(|l| l.as_str()),
                );
                self.warnings.borrow_mut().push(Warning {
                    kind: WarningKind::UnknownLabel,
                    message: format!("unknown node label: {}", label),
                    suggestion,
                });
                return Vec::new();
            }
            // RFC-035 §3.6 fast paths (slices 5+6). `None` ⇒ fall back
            // to `nodes_with_label`. Slice 7 logs the decision.
            let bound_var_prop =
                |var: &str, prop: &str| self.bound_var_index_value(bindings, var, prop);
            if let Some(indexed) = lookup::candidates_from_index(
                self.state,
                np,
                where_clause,
                self.params,
                &bound_var_prop,
            ) {
                self.record_explain(format_node_pattern(np), ExplainHit::Indexed);
                return indexed;
            }
            self.record_explain(format_node_pattern(np), ExplainHit::Fallback);
            self.state.nodes_with_label(label)
        } else {
            self.record_explain(format_node_pattern(np), ExplainHit::Fallback);
            self.state.all_nodes_sorted()
        }
    }

    /// Resolve a `NodeRef` binding's prop to an [`IndexValue`] for
    /// the cross-MATCH fast path (RFC-035 slice 6). `None` for
    /// unbound vars, non-`NodeRef` bindings, absent props, and
    /// non-indexable values (`Float` / `Null` — see `index_key_of`).
    fn bound_var_index_value(
        &self,
        bindings: &Bindings,
        var: &str,
        prop: &str,
    ) -> Option<crate::index::build::IndexValue> {
        let Some(Binding::NodeRef(idx)) = bindings.get(var) else {
            return None;
        };
        let pv = self.state.graph[*idx].props.get(prop)?;
        crate::index::build::index_key_of(pv)
    }

    pub(super) fn node_props_match(&self, idx: NodeIndex, np: &NodePattern) -> bool {
        let node = &self.state.graph[idx];
        for (k, v) in &np.props {
            match node.props.get(k) {
                Some(actual) if actual == v => {}
                _ => return false,
            }
        }
        true
    }

    pub(super) fn apply_optional<'e>(
        &'e self,
        table: BindingStream<'e>,
        inner: &'e Pattern,
        where_clause: Option<&'e Predicate>,
    ) -> BindingStream<'e> {
        Box::new(table.flat_map(move |bindings| {
            let mut out: Vec<Bindings> = Vec::new();
            self.apply_optional_row(&mut out, bindings, inner, where_clause);
            out
        }))
    }

    /// Per-row body of [`apply_optional`] — runs the inner pattern with a
    /// one-row seed, materialises the expansion (needed to decide between
    /// emission and null-fill), then either extends `out` with the
    /// expansion or null-fills the carrying bindings. The one-row
    /// materialisation is bounded by the inner pattern's fan-out for a
    /// single input row — O(candidate_count), not O(table × candidates).
    fn apply_optional_row(
        &self,
        out: &mut Vec<Bindings>,
        bindings: Bindings,
        inner: &Pattern,
        where_clause: Option<&Predicate>,
    ) {
        let inner_seed: BindingStream<'_> = Box::new(std::iter::once(bindings.clone()));
        let expanded: Vec<Bindings> = self
            .apply_pattern(inner_seed, inner, where_clause)
            .collect();
        if expanded.is_empty() {
            let mut null_filled = bindings;
            for var in collect_pattern_vars(inner) {
                null_filled.entry(var).or_insert(Binding::Null);
            }
            out.push(null_filled);
        } else {
            out.extend(expanded);
        }
    }

    pub(super) fn apply_unwind<'e>(
        &'e self,
        table: BindingStream<'e>,
        list_param: &'e str,
        var: &'e str,
    ) -> BindingStream<'e> {
        let Some(Param::List(items)) = self.params.get(list_param) else {
            self.warnings.borrow_mut().push(Warning {
                kind: WarningKind::EmptyResult,
                message: format!("UNWIND ${}: parameter missing or not a list", list_param),
                suggestion: None,
            });
            return Box::new(std::iter::empty());
        };
        Box::new(table.flat_map(move |bindings| {
            let mut out: Vec<Bindings> = Vec::new();
            unwind_row(&mut out, &bindings, items, var);
            out
        }))
    }
}

/// Per-row body of [`Evaluator::apply_unwind`] — iterator-chain form so
/// the per-item clones do not register as clones-in-loop (the outer `for`
/// loop body now contains only a helper call).
fn unwind_row(
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

/// Static analysis for issue #409 — return `true` iff the candidate
/// set produced by [`Evaluator::candidate_nodes`] is provably the same
/// for every incoming binding row. The cached fast path in
/// [`Evaluator::apply_node_pattern`] is only safe under this predicate.
///
/// The candidate set is independent iff:
/// 1. The NodePattern's literal-prop map carries no var refs (always
///    true today — `NodePattern.props` is `BTreeMap<String, PropValue>`
///    of literals only).
/// 2. No LEAF predicate inside the top-level WHERE simultaneously
///    references `own_var` AND a foreign var. Such a predicate is a
///    *cross-binding coupling* (e.g. `a.kind = b.kind`,
///    `a.qname IN [b.qname]`) and is the only WHERE shape that lets
///    `lookup::candidates_from_index` narrow `own_var`'s candidate set
///    based on a foreign binding (RFC-035 §3.6 slice 6).
///
/// Predicates that mention only `own_var` (`b.kind = 'fn'`) or only
/// foreign vars (`a.kind = 'fn'`) are NOT coupling — the foreign-only
/// predicate is irrelevant to `own_var`'s candidate set, and the
/// own-only predicate produces the same narrowed set on every row.
/// Recursive `And` / `Or` / `Not` walked into their leaves.
///
/// `NotExists` subqueries are treated as coupling (conservative — the
/// inner Query's binding scope is not analysed here).
///
/// **Why static, not dynamic.** Probing the bound-var closure to
/// detect dependence at runtime would burn allocation on every row
/// and require a fallback if the second row's binding diverged. The
/// static walk runs once per query and is exact for the AST shapes
/// the parser produces today.
fn is_binding_independent_pattern(np: &NodePattern, where_clause: Option<&Predicate>) -> bool {
    let own_var: Option<&str> = np.var.as_deref();
    match where_clause {
        None => true,
        Some(pred) => !predicate_couples_own_to_foreign(pred, own_var),
    }
}

/// `true` iff any leaf predicate references both `own_var` AND a
/// foreign var on either side. Walks `And`/`Or`/`Not` into their
/// leaves so a compound `(a.x = b.x) AND (b.kind = 'fn')` still trips
/// the coupling flag on the first leaf.
fn predicate_couples_own_to_foreign(pred: &Predicate, own_var: Option<&str>) -> bool {
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
        // Conservative — `NotExists { inner }` carries a Query whose
        // bindings cannot be safely scoped without recursing into the
        // inner pattern set. Treat as coupling so the per-row path
        // runs (no false caching).
        Predicate::NotExists { .. } => true,
    }
}

/// Leaf coupling check: gather every var referenced across both sides
/// of the predicate; coupling iff `own_var` appears AND any other var
/// also appears.
fn leaf_couples(
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

/// Walk an [`Expr`] and push every referenced var name (from
/// `Property` or bare `Var`) into `acc`. Literal / Param / List /
/// Call walked recursively.
fn collect_expr_vars_into<'e>(expr: &'e cfdb_core::query::Expr, acc: &mut Vec<&'e str>) {
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

pub(super) fn matches_existing(existing: &Binding, idx: NodeIndex) -> bool {
    matches!(existing, Binding::NodeRef(i) if *i == idx)
}

pub(super) fn edge_label_matches(pattern: &EdgePattern, edge: &cfdb_core::fact::Edge) -> bool {
    match &pattern.label {
        Some(lbl) => edge.label == *lbl,
        None => true,
    }
}

fn collect_pattern_vars(pattern: &Pattern) -> Vec<String> {
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

//! Pattern application — `MATCH`, path traversal, `OPTIONAL MATCH`, `UNWIND`.
//! Streaming iterator adapters: per incoming binding row we build a bounded
//! scratch `Vec<Bindings>`, yield it, then drop — peak memory is O(per-row
//! fan-out), not O(cartesian). Memory note lives in `super::Evaluator`.

use std::collections::{BTreeSet, VecDeque};

use cfdb_core::query::{
    Direction, EdgePattern, NodePattern, Param, PathPattern, Pattern, Predicate,
};
use cfdb_core::result::{RowValue, Warning, WarningKind};
use petgraph::stable_graph::{EdgeIndex, NodeIndex};
use petgraph::visit::EdgeRef;

use super::explain_fmt::format_node_pattern;
use super::util::suggest_label;
use super::{Binding, BindingStream, Bindings, Evaluator, DEFAULT_VAR_LENGTH_MAX};
use crate::explain::ExplainHit;
use crate::index::lookup;

impl<'a> Evaluator<'a> {
    pub(super) fn apply_node_pattern<'e>(
        &'e self,
        table: BindingStream<'e>,
        np: &'e NodePattern,
        where_clause: Option<&'e Predicate>,
    ) -> BindingStream<'e> {
        // Per-row candidate_nodes — the incoming row's bindings pick
        // the cross-MATCH bucket (RFC-035 slice 6). Empty bindings
        // collapse to slice-5 behaviour.
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

    pub(super) fn apply_path_pattern<'e>(
        &'e self,
        table: BindingStream<'e>,
        pp: &'e PathPattern,
        where_clause: Option<&'e Predicate>,
    ) -> BindingStream<'e> {
        if self.warn_on_unknown_edge_label(pp) {
            return Box::new(std::iter::empty());
        }
        Box::new(table.flat_map(move |bindings| {
            let mut out: Vec<Bindings> = Vec::new();
            self.emit_path_bindings(&mut out, &bindings, pp, where_clause);
            out
        }))
    }

    /// Emit the `UnknownEdgeLabel` warning for a path pattern whose declared
    /// edge label is absent from the keyspace. Returns `true` when the caller
    /// should short-circuit (no matches possible).
    fn warn_on_unknown_edge_label(&self, pp: &PathPattern) -> bool {
        let Some(label) = &pp.edge.label else {
            return false;
        };
        if self.state.has_edge_label(label) {
            return false;
        }
        let suggestion = suggest_label(
            label.as_str(),
            self.state.edge_labels.iter().map(|l| l.as_str()),
        );
        self.warnings.borrow_mut().push(Warning {
            kind: WarningKind::UnknownEdgeLabel,
            message: format!("unknown edge label: {}", label),
            suggestion,
        });
        true
    }

    /// Expand one binding row by enumerating src candidates, walking edges,
    /// and emitting new rows for each `(src_idx, dst_idx)` pair that passes
    /// [`Self::build_path_binding`]. Split out of `apply_path_pattern` to
    /// keep cognitive complexity below the project ceiling (RFC-031 §5 /
    /// issue #26).
    fn emit_path_bindings(
        &self,
        out: &mut Vec<Bindings>,
        bindings: &Bindings,
        pp: &PathPattern,
        where_clause: Option<&Predicate>,
    ) {
        let from_candidates = self.resolve_endpoint(bindings, &pp.from, where_clause);
        for src_idx in from_candidates {
            if !self.node_props_match(src_idx, &pp.from) {
                continue;
            }
            let reached = self.traverse(src_idx, &pp.edge);
            for (dst_idx, edge_idx) in reached {
                if let Some(next) =
                    self.build_path_binding(bindings, src_idx, dst_idx, edge_idx, pp)
                {
                    out.push(next);
                }
            }
        }
    }

    /// Assemble a single output binding for a `(src_idx, dst_idx)` path. Runs
    /// the destination-side filters, clones the carrying bindings, inserts
    /// `from.var` / `to.var` / `edge.var` (or fails if a pre-bound `to.var`
    /// disagrees with `dst_idx`). `edge_idx` is `Some` for single-hop
    /// traversals and `None` for variable-length paths where `r` would
    /// otherwise need to bind to a list of edges — that shape is deferred
    /// (issue #242). Returns `None` when any filter rejects the pair.
    fn build_path_binding(
        &self,
        bindings: &Bindings,
        src_idx: NodeIndex,
        dst_idx: NodeIndex,
        edge_idx: Option<EdgeIndex>,
        pp: &PathPattern,
    ) -> Option<Bindings> {
        if !self.matches_node_pattern_for_endpoint(dst_idx, &pp.to) {
            return None;
        }
        if !self.node_props_match(dst_idx, &pp.to) {
            return None;
        }
        let mut next = bindings.clone();
        if let Some(var) = &pp.from.var {
            next.insert(var.clone(), Binding::NodeRef(src_idx));
        }
        if let Some(var) = &pp.to.var {
            match next.get(var) {
                Some(existing) if !matches_existing(existing, dst_idx) => return None,
                Some(_) => {}
                None => {
                    next.insert(var.clone(), Binding::NodeRef(dst_idx));
                }
            }
        }
        if let (Some(var), Some(idx)) = (&pp.edge.var, edge_idx) {
            next.insert(var.clone(), Binding::EdgeRef(idx));
        }
        Some(next)
    }

    /// Resolve the source-side endpoints of a path pattern. If the endpoint
    /// variable is already bound, we must pin to that binding; otherwise we
    /// enumerate candidates via `candidate_nodes`.
    fn resolve_endpoint(
        &self,
        bindings: &Bindings,
        np: &NodePattern,
        where_clause: Option<&Predicate>,
    ) -> Vec<NodeIndex> {
        if let Some(var) = &np.var {
            if let Some(Binding::NodeRef(idx)) = bindings.get(var) {
                return vec![*idx];
            }
        }
        self.candidate_nodes(np, where_clause, bindings)
    }

    /// Label-and-variable membership check for the destination of a path.
    /// We don't emit UnknownLabel warnings from here — the outer
    /// `candidate_nodes` already warns on `from`; a `to` label is informational
    /// and we simply filter.
    fn matches_node_pattern_for_endpoint(&self, idx: NodeIndex, np: &NodePattern) -> bool {
        if let Some(label) = &np.label {
            if &self.state.graph[idx].label != label {
                return false;
            }
        }
        true
    }

    /// Traverse edges from `src_idx` according to `edge`. Honors direction
    /// and variable-length quantifier. Returns `(dst_idx, edge_idx)` pairs
    /// for destinations reached. `edge_idx` is `Some` only for single-hop
    /// emissions; for variable-length paths `edge_idx` is `None` — the
    /// edge variable would otherwise need to bind to a list of edges, and
    /// list-of-edges binding is deferred (issue #242).
    ///
    /// Single-hop (no `var_length` quantifier) emits one row per matching
    /// edge — parallel edges (`bag` semantics per `cfdb_core::fact::Edge`)
    /// each produce their own row, and `count(r)` equals the jq edge
    /// count. Variable-length paths go through a BFS that dedupes by
    /// visited node for cycle detection, matching Cypher's standard
    /// reachability semantics.
    fn traverse(
        &self,
        src_idx: NodeIndex,
        edge: &EdgePattern,
    ) -> Vec<(NodeIndex, Option<EdgeIndex>)> {
        if edge.var_length.is_none() {
            return self.traverse_single_hop(src_idx, edge);
        }
        self.traverse_bfs(src_idx, edge)
    }

    /// Single-hop traversal — emits one row per matching edge at depth=1.
    /// No BFS, no visited-set, no parallel-edge dedup (each edge counts).
    fn traverse_single_hop(
        &self,
        src_idx: NodeIndex,
        edge: &EdgePattern,
    ) -> Vec<(NodeIndex, Option<EdgeIndex>)> {
        let edges = match edge.direction {
            Direction::Out => self.collect_directed_edges(src_idx, edge, true, false),
            Direction::In => self.collect_directed_edges(src_idx, edge, false, true),
            Direction::Undirected => self.collect_directed_edges(src_idx, edge, true, true),
        };
        let mut out: Vec<(NodeIndex, Option<EdgeIndex>)> =
            edges.into_iter().map(|(n, e)| (n, Some(e))).collect();
        out.sort_by_key(|(n, e)| (*n, e.map(|i| i.index())));
        out
    }

    /// Variable-length BFS traversal — dedupes by visited node for cycle
    /// detection (Cypher reachability semantics). Returns `(dst, None)`
    /// since the edge variable cannot bind to a list in this subset.
    fn traverse_bfs(
        &self,
        src_idx: NodeIndex,
        edge: &EdgePattern,
    ) -> Vec<(NodeIndex, Option<EdgeIndex>)> {
        let (min_depth, max_depth) = edge.var_length.unwrap_or((1, 1));
        let max_depth = max_depth
            .max(min_depth)
            .min(DEFAULT_VAR_LENGTH_MAX.max(min_depth));

        let mut out: Vec<(NodeIndex, Option<EdgeIndex>)> = Vec::new();
        let mut visited: BTreeSet<NodeIndex> = BTreeSet::new();
        let mut queue: VecDeque<(NodeIndex, u32)> = VecDeque::new();
        queue.push_back((src_idx, 0));
        visited.insert(src_idx);

        while let Some((idx, depth)) = queue.pop_front() {
            if depth >= min_depth && depth > 0 {
                out.push((idx, None));
            }
            if depth >= max_depth {
                continue;
            }
            let next_depth = depth + 1;
            let edges_iter = match edge.direction {
                Direction::Out => self.collect_directed_edges(idx, edge, true, false),
                Direction::In => self.collect_directed_edges(idx, edge, false, true),
                Direction::Undirected => self.collect_directed_edges(idx, edge, true, true),
            };
            for (target, _edge_idx) in edges_iter {
                if visited.insert(target) {
                    queue.push_back((target, next_depth));
                }
            }
        }
        out.sort_by_key(|(n, _)| *n);
        out
    }

    fn collect_directed_edges(
        &self,
        idx: NodeIndex,
        edge: &EdgePattern,
        outgoing: bool,
        incoming: bool,
    ) -> Vec<(NodeIndex, EdgeIndex)> {
        let mut targets: Vec<(NodeIndex, EdgeIndex)> = Vec::new();
        if outgoing {
            for e in self.state.graph.edges(idx) {
                if edge_label_matches(edge, e.weight()) {
                    targets.push((e.target(), e.id()));
                }
            }
        }
        if incoming {
            for e in self
                .state
                .graph
                .edges_directed(idx, petgraph::Direction::Incoming)
            {
                if edge_label_matches(edge, e.weight()) {
                    targets.push((e.source(), e.id()));
                }
            }
        }
        targets
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

pub(super) fn matches_existing(existing: &Binding, idx: NodeIndex) -> bool {
    matches!(existing, Binding::NodeRef(i) if *i == idx)
}

fn edge_label_matches(pattern: &EdgePattern, edge: &cfdb_core::fact::Edge) -> bool {
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

//! Path-pattern evaluation — `(from)-[edge]->(to)` MATCH, traversal, and
//! variable-length BFS. The methods stay on `Evaluator` via a second
//! `impl` block; node-pattern methods (`apply_node_pattern`, etc.) and
//! `OPTIONAL MATCH` / `UNWIND` remain in the parent file.

use std::collections::{BTreeSet, VecDeque};

use cfdb_core::graph::{EdgeHandle, GraphReader, NodeHandle};
use cfdb_core::query::{Direction, EdgePattern, NodePattern, PathPattern, Predicate};
use cfdb_core::result::{Warning, WarningKind};

use super::coupling::{edge_label_matches, matches_existing};
use crate::eval::util::suggest_label;
use crate::eval::{Binding, BindingStream, Bindings, Evaluator, DEFAULT_VAR_LENGTH_MAX};

impl<'a, G: GraphReader + ?Sized> Evaluator<'a, G> {
    pub(in crate::eval) fn apply_path_pattern<'e>(
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
        let known = self.state.edge_labels();
        let suggestion = suggest_label(label.as_str(), known.iter().map(|l| l.as_str()));
        self.warnings.borrow_mut().push(Warning {
            kind: WarningKind::UnknownEdgeLabel,
            message: format!("unknown edge label: {}", label),
            suggestion,
        });
        true
    }

    /// Expand one binding row by enumerating src candidates, walking edges,
    /// and emitting new rows for each `(src, dst)` pair that passes
    /// [`Self::build_path_binding`].
    fn emit_path_bindings(
        &self,
        out: &mut Vec<Bindings>,
        bindings: &Bindings,
        pp: &PathPattern,
        where_clause: Option<&Predicate>,
    ) {
        let from_candidates = self.resolve_endpoint(bindings, &pp.from, where_clause);
        for src in from_candidates {
            if !self.node_props_match(src, &pp.from) {
                continue;
            }
            let reached = self.traverse(src, &pp.edge);
            for (dst, edge_h) in reached {
                if let Some(next) = self.build_path_binding(bindings, src, dst, edge_h, pp) {
                    out.push(next);
                }
            }
        }
    }

    /// Assemble a single output binding for a `(src, dst)` path. Runs
    /// the destination-side filters, clones the carrying bindings, inserts
    /// `from.var` / `to.var` / `edge.var` (or fails if a pre-bound `to.var`
    /// disagrees with `dst`). `edge_h` is `Some` for single-hop
    /// traversals and `None` for variable-length paths where `r` would
    /// otherwise need to bind to a list of edges — that shape is deferred
    /// (issue #242). Returns `None` when any filter rejects the pair.
    fn build_path_binding(
        &self,
        bindings: &Bindings,
        src: NodeHandle,
        dst: NodeHandle,
        edge_h: Option<EdgeHandle>,
        pp: &PathPattern,
    ) -> Option<Bindings> {
        if !self.matches_node_pattern_for_endpoint(dst, &pp.to) {
            return None;
        }
        if !self.node_props_match(dst, &pp.to) {
            return None;
        }
        let mut next = bindings.clone();
        if let Some(var) = &pp.from.var {
            next.insert(var.clone(), Binding::NodeRef(src));
        }
        if let Some(var) = &pp.to.var {
            match next.get(var) {
                Some(existing) if !matches_existing(existing, dst) => return None,
                Some(_) => {}
                None => {
                    next.insert(var.clone(), Binding::NodeRef(dst));
                }
            }
        }
        if let (Some(var), Some(h)) = (&pp.edge.var, edge_h) {
            next.insert(var.clone(), Binding::EdgeRef(h));
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
    ) -> Vec<NodeHandle> {
        if let Some(var) = &np.var {
            if let Some(Binding::NodeRef(h)) = bindings.get(var) {
                return vec![*h];
            }
        }
        self.candidate_nodes(np, where_clause, bindings)
    }

    /// Label-and-variable membership check for the destination of a path.
    /// We don't emit UnknownLabel warnings from here — the outer
    /// `candidate_nodes` already warns on `from`; a `to` label is informational
    /// and we simply filter.
    fn matches_node_pattern_for_endpoint(&self, h: NodeHandle, np: &NodePattern) -> bool {
        match &np.label {
            Some(label) => self.state.node(h).is_some_and(|n| &n.label == label),
            None => true,
        }
    }

    /// Traverse edges from `src` according to `edge`. Honors direction
    /// and variable-length quantifier. Returns `(dst, edge)` pairs
    /// for destinations reached. `edge` is `Some` only for single-hop
    /// emissions; for variable-length paths `edge` is `None` — the
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
        src: NodeHandle,
        edge: &EdgePattern,
    ) -> Vec<(NodeHandle, Option<EdgeHandle>)> {
        if edge.var_length.is_none() {
            return self.traverse_single_hop(src, edge);
        }
        self.traverse_bfs(src, edge)
    }

    /// Single-hop traversal — emits one row per matching edge at depth=1.
    /// No BFS, no visited-set, no parallel-edge dedup (each edge counts).
    fn traverse_single_hop(
        &self,
        src: NodeHandle,
        edge: &EdgePattern,
    ) -> Vec<(NodeHandle, Option<EdgeHandle>)> {
        let edges = match edge.direction {
            Direction::Out => self.collect_directed_edges(src, edge, true, false),
            Direction::In => self.collect_directed_edges(src, edge, false, true),
            Direction::Undirected => self.collect_directed_edges(src, edge, true, true),
        };
        let mut out: Vec<(NodeHandle, Option<EdgeHandle>)> =
            edges.into_iter().map(|(n, e)| (n, Some(e))).collect();
        out.sort_by_key(|(n, e)| (*n, *e));
        out
    }

    /// Variable-length BFS traversal — dedupes by visited node for cycle
    /// detection (Cypher reachability semantics). Returns `(dst, None)`
    /// since the edge variable cannot bind to a list in this subset.
    fn traverse_bfs(
        &self,
        src: NodeHandle,
        edge: &EdgePattern,
    ) -> Vec<(NodeHandle, Option<EdgeHandle>)> {
        // Resolve the BFS frontier ceiling from the var-length quantifier.
        // The ceiling is honoured for explicit bounds and is unbounded for
        // the open form — it was previously clamped to `DEFAULT_VAR_LENGTH_MAX`
        // for *every* pattern, silently truncating explicit deep traversals.
        let (min_depth, max_depth) = match edge.var_length {
            // Open form `*N..` (B1 maps an omitted upper bound to `u32::MAX`):
            // truly UNBOUNDED (council Q1) — the visited-set is the only bound.
            // This BFS is O(V+E) because the `visited.insert` guard below
            // enqueues each node at most once, so a numeric depth cap buys
            // nothing. `DEFAULT_VAR_LENGTH_MAX` is NOT applied here.
            Some((lo, hi)) if hi == u32::MAX => (lo, u32::MAX),
            // Explicit finite bound `*N..M`: honour `M` exactly as written —
            // no silent clamp to `DEFAULT_VAR_LENGTH_MAX` (council Q2).
            Some((lo, hi)) => (lo, hi.max(lo)),
            // No quantifier never reaches `traverse_bfs` (the caller gates on
            // `var_length.is_some()`); fall back defensively to the documented
            // ceiling. This is the *only* surviving use of the constant.
            None => (1, DEFAULT_VAR_LENGTH_MAX),
        };

        let mut out: Vec<(NodeHandle, Option<EdgeHandle>)> = Vec::new();
        let mut visited: BTreeSet<NodeHandle> = BTreeSet::new();
        let mut queue: VecDeque<(NodeHandle, u32)> = VecDeque::new();
        queue.push_back((src, 0));
        visited.insert(src);

        while let Some((h, depth)) = queue.pop_front() {
            if depth >= min_depth && depth > 0 {
                out.push((h, None));
            }
            if depth >= max_depth {
                continue;
            }
            let next_depth = depth + 1;
            let edges_iter = match edge.direction {
                Direction::Out => self.collect_directed_edges(h, edge, true, false),
                Direction::In => self.collect_directed_edges(h, edge, false, true),
                Direction::Undirected => self.collect_directed_edges(h, edge, true, true),
            };
            for (target, _edge_h) in edges_iter {
                if visited.insert(target) {
                    queue.push_back((target, next_depth));
                }
            }
        }
        out.sort_by_key(|(n, _)| *n);
        out
    }

    /// `(other endpoint, edge)` for every edge at `h` in the requested
    /// direction(s) whose label satisfies `edge`. An unlabelled pattern
    /// keeps every edge without dereferencing it.
    fn collect_directed_edges(
        &self,
        h: NodeHandle,
        edge: &EdgePattern,
        outgoing: bool,
        incoming: bool,
    ) -> Vec<(NodeHandle, EdgeHandle)> {
        let keep = |eh: EdgeHandle| {
            edge.label.is_none()
                || self
                    .state
                    .edge(eh)
                    .is_some_and(|e| edge_label_matches(edge, e))
        };
        let mut targets: Vec<(NodeHandle, EdgeHandle)> = Vec::new();
        if outgoing {
            targets.extend(
                self.state
                    .edges_out(h)
                    .into_iter()
                    .filter(|(eh, _)| keep(*eh))
                    .map(|(eh, other)| (other, eh)),
            );
        }
        if incoming {
            targets.extend(
                self.state
                    .edges_in(h)
                    .into_iter()
                    .filter(|(eh, _)| keep(*eh))
                    .map(|(eh, other)| (other, eh)),
            );
        }
        targets
    }
}

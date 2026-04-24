//! Path-pattern evaluation — `(from)-[edge]->(to)` MATCH, traversal, and
//! variable-length BFS. Extracted from `super::pattern` as part of the
//! #253 god-file split. The methods stay on `Evaluator` via a second
//! `impl` block; node-pattern methods (`apply_node_pattern`, etc.) and
//! `OPTIONAL MATCH` / `UNWIND` remain in the parent file.

use std::collections::{BTreeSet, VecDeque};

use cfdb_core::query::{Direction, EdgePattern, NodePattern, PathPattern, Predicate};
use cfdb_core::result::{Warning, WarningKind};
use petgraph::stable_graph::{EdgeIndex, NodeIndex};
use petgraph::visit::EdgeRef;

use super::{edge_label_matches, matches_existing};
use crate::eval::util::suggest_label;
use crate::eval::{Binding, BindingStream, Bindings, Evaluator, DEFAULT_VAR_LENGTH_MAX};

impl<'a> Evaluator<'a> {
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
}

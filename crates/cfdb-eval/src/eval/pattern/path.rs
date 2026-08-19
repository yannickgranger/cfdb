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

    fn matches_node_pattern_for_endpoint(&self, h: NodeHandle, np: &NodePattern) -> bool {
        match &np.label {
            Some(label) => self.state.node(h).is_some_and(|n| &n.label == label),
            None => true,
        }
    }

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

    fn traverse_bfs(
        &self,
        src: NodeHandle,
        edge: &EdgePattern,
    ) -> Vec<(NodeHandle, Option<EdgeHandle>)> {
        let (min_depth, max_depth) = match edge.var_length {
            Some((lo, hi)) if hi == u32::MAX => (lo, u32::MAX),
            Some((lo, hi)) => (lo, hi.max(lo)),
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

//! Pattern application — `MATCH`, path traversal, `OPTIONAL MATCH`, `UNWIND`.
//!
//! These methods expand the binding table by joining it against the matches
//! produced by each pattern. See `super::Evaluator::run` for the pipeline
//! order.

use std::collections::{BTreeSet, VecDeque};

use cfdb_core::query::{Direction, EdgePattern, NodePattern, Param, PathPattern, Pattern};
use cfdb_core::result::{RowValue, Warning, WarningKind};
use petgraph::stable_graph::NodeIndex;
use petgraph::visit::EdgeRef;

use super::util::suggest_label;
use super::{Binding, Bindings, Evaluator, DEFAULT_VAR_LENGTH_MAX};

impl<'a> Evaluator<'a> {
    pub(super) fn apply_node_pattern(
        &mut self,
        table: Vec<Bindings>,
        np: &NodePattern,
    ) -> Vec<Bindings> {
        let candidates = self.candidate_nodes(np);
        let mut out: Vec<Bindings> = Vec::new();
        for bindings in table {
            if let Some(var) = &np.var {
                if let Some(existing) = bindings.get(var) {
                    for idx in &candidates {
                        if matches_existing(existing, *idx) && self.node_props_match(*idx, np) {
                            out.push(bindings.clone());
                            break;
                        }
                    }
                } else {
                    for idx in &candidates {
                        if !self.node_props_match(*idx, np) {
                            continue;
                        }
                        let mut next = bindings.clone();
                        next.insert(var.clone(), Binding::NodeRef(*idx));
                        out.push(next);
                    }
                }
            } else {
                for idx in &candidates {
                    if self.node_props_match(*idx, np) {
                        out.push(bindings.clone());
                    }
                }
            }
        }
        out
    }

    pub(super) fn candidate_nodes(&mut self, np: &NodePattern) -> Vec<NodeIndex> {
        if let Some(label) = &np.label {
            if !self.state.has_label(label) {
                let suggestion = suggest_label(
                    label.as_str(),
                    self.state.by_label.keys().map(|l| l.as_str()),
                );
                self.warnings.push(Warning {
                    kind: WarningKind::UnknownLabel,
                    message: format!("unknown node label: {}", label),
                    suggestion,
                });
                return Vec::new();
            }
            self.state.nodes_with_label(label)
        } else {
            self.state.all_nodes_sorted()
        }
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

    pub(super) fn apply_path_pattern(
        &mut self,
        table: Vec<Bindings>,
        pp: &PathPattern,
    ) -> Vec<Bindings> {
        if let Some(label) = &pp.edge.label {
            if !self.state.has_edge_label(label) {
                let suggestion = suggest_label(
                    label.as_str(),
                    self.state.edge_labels.iter().map(|l| l.as_str()),
                );
                self.warnings.push(Warning {
                    kind: WarningKind::UnknownEdgeLabel,
                    message: format!("unknown edge label: {}", label),
                    suggestion,
                });
                return Vec::new();
            }
        }

        let mut out: Vec<Bindings> = Vec::new();
        for bindings in table {
            let from_candidates = self.resolve_endpoint(&bindings, &pp.from);
            for src_idx in from_candidates {
                if !self.node_props_match(src_idx, &pp.from) {
                    continue;
                }
                let reached = self.traverse(src_idx, &pp.edge);
                for dst_idx in reached {
                    if !self.matches_node_pattern_for_endpoint(dst_idx, &pp.to) {
                        continue;
                    }
                    if !self.node_props_match(dst_idx, &pp.to) {
                        continue;
                    }
                    let mut next = bindings.clone();
                    if let Some(var) = &pp.from.var {
                        next.insert(var.clone(), Binding::NodeRef(src_idx));
                    }
                    if let Some(var) = &pp.to.var {
                        if let Some(existing) = next.get(var) {
                            if !matches_existing(existing, dst_idx) {
                                continue;
                            }
                        } else {
                            next.insert(var.clone(), Binding::NodeRef(dst_idx));
                        }
                    }
                    out.push(next);
                }
            }
        }
        out
    }

    /// Resolve the source-side endpoints of a path pattern. If the endpoint
    /// variable is already bound, we must pin to that binding; otherwise we
    /// enumerate candidates via `candidate_nodes`.
    fn resolve_endpoint(&mut self, bindings: &Bindings, np: &NodePattern) -> Vec<NodeIndex> {
        if let Some(var) = &np.var {
            if let Some(Binding::NodeRef(idx)) = bindings.get(var) {
                return vec![*idx];
            }
        }
        self.candidate_nodes(np)
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
    /// and variable-length quantifier. Returns the set of destination node
    /// indices reached (BFS semantics with cycle detection).
    fn traverse(&self, src_idx: NodeIndex, edge: &EdgePattern) -> Vec<NodeIndex> {
        let (min_depth, max_depth) = edge.var_length.unwrap_or((1, 1));
        let max_depth = max_depth
            .max(min_depth)
            .min(DEFAULT_VAR_LENGTH_MAX.max(min_depth));

        let mut out: Vec<NodeIndex> = Vec::new();
        let mut visited: BTreeSet<NodeIndex> = BTreeSet::new();
        let mut queue: VecDeque<(NodeIndex, u32)> = VecDeque::new();
        queue.push_back((src_idx, 0));
        visited.insert(src_idx);

        while let Some((idx, depth)) = queue.pop_front() {
            if depth >= min_depth && depth > 0 {
                out.push(idx);
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
            for target in edges_iter {
                if visited.insert(target) {
                    queue.push_back((target, next_depth));
                }
            }
        }
        out.sort();
        out
    }

    fn collect_directed_edges(
        &self,
        idx: NodeIndex,
        edge: &EdgePattern,
        outgoing: bool,
        incoming: bool,
    ) -> Vec<NodeIndex> {
        let mut targets: Vec<NodeIndex> = Vec::new();
        if outgoing {
            for e in self.state.graph.edges(idx) {
                if edge_label_matches(edge, e.weight()) {
                    targets.push(e.target());
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
                    targets.push(e.source());
                }
            }
        }
        targets
    }

    pub(super) fn apply_optional(
        &mut self,
        table: Vec<Bindings>,
        inner: &Pattern,
    ) -> Vec<Bindings> {
        let mut out: Vec<Bindings> = Vec::new();
        for bindings in table {
            let expanded = self.apply_pattern(vec![bindings.clone()], inner);
            if expanded.is_empty() {
                let mut null_filled = bindings.clone();
                for var in collect_pattern_vars(inner) {
                    null_filled.entry(var).or_insert(Binding::Null);
                }
                out.push(null_filled);
            } else {
                out.extend(expanded);
            }
        }
        out
    }

    pub(super) fn apply_unwind(
        &mut self,
        table: Vec<Bindings>,
        list_param: &str,
        var: &str,
    ) -> Vec<Bindings> {
        let Some(Param::List(items)) = self.params.get(list_param) else {
            self.warnings.push(Warning {
                kind: WarningKind::EmptyResult,
                message: format!("UNWIND ${}: parameter missing or not a list", list_param),
                suggestion: None,
            });
            return Vec::new();
        };
        let mut out: Vec<Bindings> = Vec::new();
        for bindings in table {
            for item in items {
                let mut next = bindings.clone();
                next.insert(
                    var.to_string(),
                    Binding::Value(RowValue::Scalar(item.clone())),
                );
                out.push(next);
            }
        }
        out
    }
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

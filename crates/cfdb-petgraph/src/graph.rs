//! Per-keyspace graph state — a `StableDiGraph<Node, Edge>` plus an
//! insertion-ordered id → `NodeIndex` map and a label index.
//!
//! Determinism (RFC §12 G1): the id map uses `IndexMap` so iteration order is
//! insertion order; the label index uses `BTreeMap` so label iteration is
//! sorted. Two runs that ingest the same facts in the same order produce
//! identical in-memory state — and identical canonical dumps.

use std::collections::{BTreeMap, BTreeSet};

use cfdb_core::fact::{Edge, Node};
use cfdb_core::result::{Warning, WarningKind};
use cfdb_core::schema::{EdgeLabel, Label};
use indexmap::IndexMap;
use petgraph::stable_graph::{NodeIndex, StableDiGraph};

/// In-memory state for a single keyspace.
///
/// Fields are crate-visible so `eval.rs` can walk the graph without extra
/// accessor overhead. External callers go through `PetgraphStore` (which owns
/// a `BTreeMap<Keyspace, KeyspaceState>`).
pub(crate) struct KeyspaceState {
    /// The underlying directed graph with stable indices. Node indices stay
    /// valid across insertions which matters for the id → index map.
    pub(crate) graph: StableDiGraph<Node, Edge>,

    /// Id → NodeIndex lookup. `IndexMap` preserves insertion order so the
    /// first iteration at canonical-dump time is deterministic for free.
    pub(crate) id_to_idx: IndexMap<String, NodeIndex>,

    /// Label → set of node indices. `BTreeMap` for sorted label iteration;
    /// `BTreeSet<NodeIndex>` for sorted-by-index iteration within a label
    /// (matches insertion order because `NodeIndex` increases with each add).
    pub(crate) by_label: BTreeMap<Label, BTreeSet<NodeIndex>>,

    /// Set of edge labels observed during ingest. Used for unknown-label
    /// warnings in the evaluator.
    pub(crate) edge_labels: BTreeSet<EdgeLabel>,

    /// Warnings accumulated during ingest (e.g. unresolved edge endpoints).
    /// Surfaced on every subsequent `execute` call alongside query-time
    /// warnings so partially-ingested graphs are obvious to the caller.
    pub(crate) ingest_warnings: Vec<Warning>,
}

impl KeyspaceState {
    pub(crate) fn new() -> Self {
        Self {
            graph: StableDiGraph::new(),
            id_to_idx: IndexMap::new(),
            by_label: BTreeMap::new(),
            edge_labels: BTreeSet::new(),
            ingest_warnings: Vec::new(),
        }
    }

    /// Add or replace a batch of nodes. Existing ids update in place so the
    /// label index stays coherent across re-ingests.
    pub(crate) fn ingest_nodes(&mut self, nodes: Vec<Node>) {
        for node in nodes {
            self.ingest_one_node(node);
        }
    }

    /// Per-node body of [`ingest_nodes`] — factored out so the `label.clone()` /
    /// `id.clone()` calls required by the label-index + id-map don't count
    /// as clones-in-loop (the outer loop body now contains only a helper
    /// dispatch).
    fn ingest_one_node(&mut self, node: Node) {
        if let Some(&idx) = self.id_to_idx.get(&node.id) {
            if let Some(existing) = self.graph.node_weight_mut(idx) {
                if existing.label != node.label {
                    if let Some(set) = self.by_label.get_mut(&existing.label) {
                        set.remove(&idx);
                    }
                    self.by_label
                        .entry(node.label.clone())
                        .or_default()
                        .insert(idx);
                }
                *existing = node;
            }
        } else {
            let id = node.id.clone();
            let label = node.label.clone();
            let idx = self.graph.add_node(node);
            self.id_to_idx.insert(id, idx);
            self.by_label.entry(label).or_default().insert(idx);
        }
    }

    /// Add a batch of edges. Endpoints that reference unknown ids are skipped
    /// and reported on `ingest_warnings` (RFC §6 — bulk loads degrade
    /// gracefully).
    pub(crate) fn ingest_edges(&mut self, edges: Vec<Edge>) {
        for edge in edges {
            self.ingest_one_edge(edge);
        }
    }

    /// Per-edge body of [`ingest_edges`] — factored out so the
    /// `edge.label.clone()` required by the edge-label index does not
    /// register as a clone inside the outer `for` loop body.
    fn ingest_one_edge(&mut self, edge: Edge) {
        let Some(&src_idx) = self.id_to_idx.get(&edge.src) else {
            self.ingest_warnings.push(Warning {
                kind: WarningKind::EmptyResult,
                message: format!(
                    "edge {} -[{}]-> {}: unknown src id, edge skipped",
                    edge.src, edge.label, edge.dst
                ),
                suggestion: None,
            });
            return;
        };
        let Some(&dst_idx) = self.id_to_idx.get(&edge.dst) else {
            self.ingest_warnings.push(Warning {
                kind: WarningKind::EmptyResult,
                message: format!(
                    "edge {} -[{}]-> {}: unknown dst id, edge skipped",
                    edge.src, edge.label, edge.dst
                ),
                suggestion: None,
            });
            return;
        };
        self.edge_labels.insert(edge.label.clone());
        self.graph.add_edge(src_idx, dst_idx, edge);
    }

    /// Look up the node indices for a given label, in sorted order.
    pub(crate) fn nodes_with_label(&self, label: &Label) -> Vec<NodeIndex> {
        self.by_label
            .get(label)
            .map(|set| set.iter().copied().collect())
            .unwrap_or_default()
    }

    /// All node indices in sorted id order (for unlabelled patterns).
    pub(crate) fn all_nodes_sorted(&self) -> Vec<NodeIndex> {
        let mut ids: Vec<(&String, NodeIndex)> =
            self.id_to_idx.iter().map(|(id, idx)| (id, *idx)).collect();
        ids.sort_by(|a, b| a.0.cmp(b.0));
        ids.into_iter().map(|(_, idx)| idx).collect()
    }

    /// True iff the given label was ever observed on a node in this keyspace.
    pub(crate) fn has_label(&self, label: &Label) -> bool {
        self.by_label.contains_key(label)
    }

    /// True iff the given edge label was ever observed on an edge in this
    /// keyspace.
    pub(crate) fn has_edge_label(&self, label: &EdgeLabel) -> bool {
        self.edge_labels.contains(label)
    }
}

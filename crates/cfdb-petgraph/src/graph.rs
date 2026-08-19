use std::collections::{BTreeMap, BTreeSet};

use cfdb_core::fact::{Edge, Node};
use cfdb_core::result::{Warning, WarningKind};
use cfdb_core::schema::{EdgeLabel, Label};
use indexmap::IndexMap;

use crate::ingest_contention::{detect_contention, CONTENTION_SUGGESTION, CONTENTION_WARNING_CAP};
use petgraph::stable_graph::{NodeIndex, StableDiGraph};

use crate::index::build::{entry_value_for_node, IndexTag, IndexValue};
use crate::index::spec::{IndexEntry, IndexSpec};

pub(crate) struct KeyspaceState {
    pub(crate) graph: StableDiGraph<Node, Edge>,

    pub(crate) id_to_idx: IndexMap<String, NodeIndex>,

    pub(crate) by_label: BTreeMap<Label, BTreeSet<NodeIndex>>,

    pub(crate) edge_labels: BTreeSet<EdgeLabel>,

    pub(crate) ingest_warnings: Vec<Warning>,

    pub(crate) recorded_contentions: usize,

    pub(crate) suppressed_contentions: usize,

    pub(crate) index_spec: IndexSpec,

    pub(crate) by_prop: BTreeMap<(Label, IndexTag), BTreeMap<IndexValue, BTreeSet<NodeIndex>>>,

    pub(crate) indexed_pairs: BTreeMap<String, BTreeSet<IndexTag>>,
}

impl KeyspaceState {
    #[cfg(test)]
    pub(crate) fn new() -> Self {
        Self::new_with_spec(IndexSpec::empty())
    }

    pub(crate) fn new_with_spec(spec: IndexSpec) -> Self {
        let indexed_pairs = indexed_pairs_for(&spec);
        Self {
            graph: StableDiGraph::new(),
            id_to_idx: IndexMap::new(),
            by_label: BTreeMap::new(),
            edge_labels: BTreeSet::new(),
            ingest_warnings: Vec::new(),
            recorded_contentions: 0,
            suppressed_contentions: 0,
            index_spec: spec,
            by_prop: BTreeMap::new(),
            indexed_pairs,
        }
    }

    pub(crate) fn ingest_nodes(&mut self, nodes: Vec<Node>) {
        for node in nodes {
            self.ingest_one_node(node);
        }
    }

    fn ingest_one_node(&mut self, node: Node) {
        if let Some(&idx) = self.id_to_idx.get(&node.id) {
            self.record_contention(idx, &node);
            let before: Vec<(Label, IndexTag, IndexValue)> = match self.graph.node_weight(idx) {
                Some(existing) => self.compute_index_entries(existing),
                None => return,
            };
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
            let after: Vec<(Label, IndexTag, IndexValue)> = match self.graph.node_weight(idx) {
                Some(updated) => self.compute_index_entries(updated),
                None => Vec::new(),
            };
            self.reconcile_index_entries(idx, &before, &after);
        } else {
            let id = node.id.clone();
            let label = node.label.clone();
            let entries = self.compute_index_entries(&node);
            let idx = self.graph.add_node(node);
            self.id_to_idx.insert(id, idx);
            self.by_label.entry(label).or_default().insert(idx);
            for (label, tag, value) in entries {
                self.by_prop
                    .entry((label, tag))
                    .or_default()
                    .entry(value)
                    .or_default()
                    .insert(idx);
            }
        }
    }

    fn record_contention(&mut self, idx: NodeIndex, incoming: &Node) {
        let Some(mut w) = self
            .graph
            .node_weight(idx)
            .and_then(|existing| detect_contention(existing, incoming))
        else {
            return;
        };
        if self.recorded_contentions >= CONTENTION_WARNING_CAP {
            self.suppressed_contentions += 1;
            return;
        }
        if self.recorded_contentions == 0 {
            w.suggestion = Some(CONTENTION_SUGGESTION.to_string());
        }
        self.recorded_contentions += 1;
        self.ingest_warnings.push(w);
    }

    pub(crate) fn materialized_ingest_warnings(&self) -> Vec<Warning> {
        let mut out = self.ingest_warnings.clone();
        if self.suppressed_contentions > 0 {
            out.push(Warning {
                kind: WarningKind::IdentityContention,
                message: format!(
                    "and {} further identity contention(s) suppressed (cap {})",
                    self.suppressed_contentions, CONTENTION_WARNING_CAP
                ),
                suggestion: None,
            });
        }
        out
    }

    fn compute_index_entries(&self, node: &Node) -> Vec<(Label, IndexTag, IndexValue)> {
        if self.index_spec.entries.is_empty() {
            return Vec::new();
        }
        self.index_spec
            .entries
            .iter()
            .filter_map(|entry| entry_value_for_node(entry, node))
            .collect()
    }

    fn reconcile_index_entries(
        &mut self,
        idx: NodeIndex,
        before: &[(Label, IndexTag, IndexValue)],
        after: &[(Label, IndexTag, IndexValue)],
    ) {
        let before_set: BTreeSet<_> = before.iter().collect();
        let after_set: BTreeSet<_> = after.iter().collect();

        for stale in before_set.difference(&after_set) {
            let (label, tag, value) = *stale;
            crate::index::posting::remove_posting(&mut self.by_prop, label, tag, value, idx);
        }

        for fresh in after_set.difference(&before_set) {
            let (label, tag, value) = *fresh;
            crate::index::posting::insert_posting(&mut self.by_prop, label, tag, value, idx);
        }
    }

    pub(crate) fn ingest_edges(&mut self, edges: Vec<Edge>) {
        for edge in edges {
            self.ingest_one_edge(edge);
        }
    }

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

    pub(crate) fn nodes_with_label(&self, label: &Label) -> Vec<NodeIndex> {
        self.by_label
            .get(label)
            .map(|set| set.iter().copied().collect())
            .unwrap_or_default()
    }

    pub(crate) fn all_nodes_sorted(&self) -> Vec<NodeIndex> {
        let mut ids: Vec<(&String, NodeIndex)> =
            self.id_to_idx.iter().map(|(id, idx)| (id, *idx)).collect();
        ids.sort_by(|a, b| a.0.cmp(b.0));
        ids.into_iter().map(|(_, idx)| idx).collect()
    }

    pub(crate) fn has_label(&self, label: &Label) -> bool {
        self.by_label.contains_key(label)
    }

    pub(crate) fn has_edge_label(&self, label: &EdgeLabel) -> bool {
        self.edge_labels.contains(label)
    }
}

fn indexed_pairs_for(spec: &IndexSpec) -> BTreeMap<String, BTreeSet<IndexTag>> {
    let mut out: BTreeMap<String, BTreeSet<IndexTag>> = BTreeMap::new();
    for entry in &spec.entries {
        let (label, tag) = match entry {
            IndexEntry::Prop { label, prop, .. } => (label.clone(), prop.clone()),
            IndexEntry::Computed {
                label, computed, ..
            } => (label.clone(), computed.as_str().to_string()),
        };
        out.entry(label).or_default().insert(tag);
    }
    out
}

#[cfg(test)]
mod index_build_tests;

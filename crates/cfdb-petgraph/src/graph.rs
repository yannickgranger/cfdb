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

use crate::index::build::{entry_value_for_node, IndexTag, IndexValue};
use crate::index::spec::{IndexEntry, IndexSpec};

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

    /// Inverted-index spec for this keyspace (RFC-035 slice 2 #181).
    /// Empty by default; populated via [`KeyspaceState::new_with_spec`]
    /// when the composition root (slice 7 #186) hands `.cfdb/indexes.toml`
    /// down. `ingest_one_node` consults this to maintain
    /// [`Self::by_prop`] incrementally.
    pub(crate) index_spec: IndexSpec,

    /// Inverted indexes by `(Label, tag) → value → node set`. Populated
    /// at ingest time from [`Self::index_spec`]; rebuilt on load (slice
    /// 4 #183) rather than serialised to disk (RFC-035 §3.7). Empty
    /// when `index_spec` declares no indexes.
    ///
    /// The `tag` is either the literal prop name (for `IndexEntry::Prop`)
    /// or the canonical computed-key string such as
    /// `"last_segment(qname)"` (for `IndexEntry::Computed`). See
    /// [`crate::index::build`] for the `(IndexEntry, Node) → (tag, value)`
    /// mapping.
    ///
    /// **Not part of `canonical_dump`.** Indexes are rebuild-able
    /// scratch — leaking them into the byte-stable dump would break
    /// the G1 determinism invariant (RFC-035 §4). `canonical_dump.rs`
    /// does not touch this field.
    pub(crate) by_prop: BTreeMap<(Label, IndexTag), BTreeMap<IndexValue, BTreeSet<NodeIndex>>>,

    /// Precomputed `Label.as_str() → {tag, …}` membership map derived
    /// from `index_spec.entries` at construction. Lets the fast-path
    /// hint walker (`index::lookup`) replace a per-row linear scan
    /// over `entries` with a two-step `BTreeMap`/`BTreeSet` lookup
    /// where both keys borrow `&str` (so no per-call allocation).
    /// Kept in sync with `index_spec` because all construction routes
    /// through [`Self::new_with_spec`] and `index_spec` is otherwise
    /// not mutated post-construction.
    pub(crate) indexed_pairs: BTreeMap<String, BTreeSet<IndexTag>>,
}

impl KeyspaceState {
    #[cfg(test)]
    pub(crate) fn new() -> Self {
        Self::new_with_spec(IndexSpec::empty())
    }

    /// Construct a fresh keyspace bound to the given [`IndexSpec`].
    /// Subsequent [`Self::ingest_nodes`] calls walk the spec and
    /// populate [`Self::by_prop`] per RFC-035 §3.5. An empty spec is
    /// equivalent to [`Self::new`] — no index maintenance happens.
    pub(crate) fn new_with_spec(spec: IndexSpec) -> Self {
        let indexed_pairs = indexed_pairs_for(&spec);
        Self {
            graph: StableDiGraph::new(),
            id_to_idx: IndexMap::new(),
            by_label: BTreeMap::new(),
            edge_labels: BTreeSet::new(),
            ingest_warnings: Vec::new(),
            index_spec: spec,
            by_prop: BTreeMap::new(),
            indexed_pairs,
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
            // Snapshot pre-update index entries via an immutable graph
            // borrow so we can reconcile `by_prop` after the mutation
            // without fighting the borrow-checker over `self.graph`.
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

    /// Collect every `(label, tag, value)` tuple that the spec says this
    /// node should contribute to `by_prop`. A node with no matching spec
    /// entries yields an empty `Vec`. Order is spec order, which is
    /// deterministic (TOML document order preserved on parse).
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

    /// Reconcile `by_prop` for a node that was updated in place. Entries
    /// present in `before` but not `after` are removed; entries present
    /// in `after` but not `before` are inserted; unchanged entries are
    /// left alone. Empty posting lists (and empty `(label, tag)` outer
    /// entries) are pruned so iteration stays minimal.
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

/// Project an [`IndexSpec`] to the `label → {tag, …}` membership map
/// consumed by `index::lookup`. Built once at construction time so
/// the per-row hint walker can replace its linear scan over `entries`
/// with a two-step `BTreeMap`/`BTreeSet` lookup. Keys are owned
/// `String` (label) and `IndexTag` (already `String`) to let the
/// lookup borrow `&str` on the query side.
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

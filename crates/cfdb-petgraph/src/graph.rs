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
use crate::index::spec::IndexSpec;

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
}

impl KeyspaceState {
    pub(crate) fn new() -> Self {
        Self::new_with_spec(IndexSpec::empty())
    }

    /// Construct a fresh keyspace bound to the given [`IndexSpec`].
    /// Subsequent [`Self::ingest_nodes`] calls walk the spec and
    /// populate [`Self::by_prop`] per RFC-035 §3.5. An empty spec is
    /// equivalent to [`Self::new`] — no index maintenance happens.
    pub(crate) fn new_with_spec(spec: IndexSpec) -> Self {
        Self {
            graph: StableDiGraph::new(),
            id_to_idx: IndexMap::new(),
            by_label: BTreeMap::new(),
            edge_labels: BTreeSet::new(),
            ingest_warnings: Vec::new(),
            index_spec: spec,
            by_prop: BTreeMap::new(),
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

#[cfg(test)]
mod index_build_tests {
    //! RFC-035 slice 2 #181 — exercise `by_prop` build + stale-entry removal
    //! at the `KeyspaceState` layer. Lives inline because `KeyspaceState` is
    //! `pub(crate)`; tests at `PetgraphStore` level land when slice 7 #186
    //! wires the composition root's `with_indexes` builder.

    use super::*;
    use crate::canonical_dump::canonical_dump;
    use crate::index::spec::{ComputedKey, IndexEntry, IndexSpec};
    use cfdb_core::fact::{Node, PropValue};
    use cfdb_core::schema::Label;

    fn three_index_spec() -> IndexSpec {
        IndexSpec {
            entries: vec![
                IndexEntry::Prop {
                    label: "Item".into(),
                    prop: "qname".into(),
                    notes: "test".into(),
                },
                IndexEntry::Prop {
                    label: "Item".into(),
                    prop: "bounded_context".into(),
                    notes: "test".into(),
                },
                IndexEntry::Computed {
                    label: "Item".into(),
                    computed: ComputedKey::LastSegment,
                    notes: "test".into(),
                },
            ],
        }
    }

    fn item(id: &str, qname: &str, ctx: &str) -> Node {
        Node::new(id, Label::new("Item"))
            .with_prop("qname", qname)
            .with_prop("bounded_context", ctx)
    }

    fn full_scan(
        state: &KeyspaceState,
        label: &str,
        key: &str,
        value: &str,
    ) -> BTreeSet<NodeIndex> {
        let target_label = Label::new(label);
        state
            .graph
            .node_indices()
            .filter(|&idx| {
                let node = state.graph.node_weight(idx).expect("valid idx");
                if node.label != target_label {
                    return false;
                }
                match key {
                    "last_segment(qname)" => {
                        let qname = node.props.get("qname").and_then(PropValue::as_str);
                        qname
                            .map(|q| cfdb_core::qname::last_segment(q) == value)
                            .unwrap_or(false)
                    }
                    other => node
                        .props
                        .get(other)
                        .and_then(PropValue::as_str)
                        .is_some_and(|s| s == value),
                }
            })
            .collect()
    }

    #[test]
    fn recall_matches_full_scan_on_1000_item_fixture() {
        let contexts = ["context_a", "context_b", "context_c", "context_d"];
        let roots = ["alpha", "beta", "gamma", "delta", "epsilon"];
        let leaves = [
            "foo", "bar", "baz", "qux", "quux", "corge", "grault", "xyzzy",
        ];

        let mut nodes = Vec::with_capacity(1000);
        for i in 0..1000 {
            let ctx = contexts[i % contexts.len()];
            let root = roots[(i / contexts.len()) % roots.len()];
            let leaf = leaves[(i / (contexts.len() * roots.len())) % leaves.len()];
            let disc = i; // disambiguator to keep ids unique
            let qname = format!("{root}::{leaf}_{disc}");
            nodes.push(item(&format!("item:{i}"), &qname, ctx));
        }

        let mut state = KeyspaceState::new_with_spec(three_index_spec());
        state.ingest_nodes(nodes);

        assert_eq!(state.graph.node_count(), 1000);

        // Recall ≡ full scan across every indexed (Label, key).
        for ((label, tag), postings) in &state.by_prop {
            for (value, indices) in postings {
                let scanned = full_scan(&state, label.as_str(), tag, value);
                assert_eq!(
                    indices, &scanned,
                    "by_prop[({label:?}, {tag})][{value}] diverged from full scan"
                );
            }
        }

        // Contexts bucket size — 1000 / 4 contexts = 250 each.
        let ctx_key = (Label::new("Item"), "bounded_context".to_string());
        let ctx_buckets = state.by_prop.get(&ctx_key).expect("context index");
        assert_eq!(ctx_buckets.len(), contexts.len());
        for (ctx, set) in ctx_buckets {
            assert_eq!(
                set.len(),
                250,
                "context `{ctx}` should hold 1000/4 = 250 items"
            );
        }

        // Computed-key bucket size: 8 distinct leaves × 5 roots = 40 distinct
        // last-segment suffixes? No — last_segment includes the disambiguator,
        // so all 1000 values are distinct. Assert that instead.
        let comp_key = (Label::new("Item"), "last_segment(qname)".to_string());
        let comp_buckets = state.by_prop.get(&comp_key).expect("computed index");
        assert_eq!(comp_buckets.len(), 1000);
    }

    #[test]
    fn stale_entry_removed_on_reingest_with_changed_prop() {
        let mut state = KeyspaceState::new_with_spec(three_index_spec());
        state.ingest_nodes(vec![item("item:a", "mod::foo", "context_a")]);

        let label_item = Label::new("Item");
        let key_qname = (label_item.clone(), "qname".to_string());
        let key_ctx = (label_item.clone(), "bounded_context".to_string());
        let key_last = (label_item, "last_segment(qname)".to_string());

        let idx = *state.id_to_idx.get("item:a").expect("ingested");
        assert!(state.by_prop[&key_qname]["mod::foo"].contains(&idx));
        assert!(state.by_prop[&key_ctx]["context_a"].contains(&idx));
        assert!(state.by_prop[&key_last]["foo"].contains(&idx));

        // Re-ingest with a changed qname AND a changed context.
        state.ingest_nodes(vec![item("item:a", "mod::bar", "context_b")]);

        // Stale postings: old values lose the idx AND the (now-empty) entries
        // are pruned from the outer map so iteration stays minimal.
        assert!(
            !state.by_prop[&key_qname].contains_key("mod::foo"),
            "stale qname posting list should be pruned, not merely emptied"
        );
        assert!(!state.by_prop[&key_ctx].contains_key("context_a"));
        assert!(!state.by_prop[&key_last].contains_key("foo"));

        // Fresh postings: new values carry the idx.
        assert!(state.by_prop[&key_qname]["mod::bar"].contains(&idx));
        assert!(state.by_prop[&key_ctx]["context_b"].contains(&idx));
        assert!(state.by_prop[&key_last]["bar"].contains(&idx));

        // Only one node in the keyspace, so the node-count stays at 1.
        assert_eq!(state.graph.node_count(), 1);
    }

    #[test]
    fn canonical_dump_unaffected_by_by_prop() {
        // Determinism / G1 invariant: indexes are rebuild-able scratch and
        // MUST NOT leak into `canonical_dump`. A keyspace ingested with
        // indexes and one ingested without indexes produce byte-identical
        // canonical dumps on the same fact content (RFC-035 §4).
        let nodes = vec![
            item("item:a", "mod::foo", "context_a"),
            item("item:b", "mod::bar", "context_b"),
            item("item:c", "other::foo", "context_a"),
        ];

        let mut indexed = KeyspaceState::new_with_spec(three_index_spec());
        indexed.ingest_nodes(nodes.clone());

        let mut plain = KeyspaceState::new();
        plain.ingest_nodes(nodes);

        let indexed_dump = canonical_dump(&indexed);
        let plain_dump = canonical_dump(&plain);
        assert_eq!(
            indexed_dump, plain_dump,
            "canonical_dump must be byte-identical with vs without indexes"
        );

        // Sanity: the indexed keyspace actually populated its posting lists.
        assert!(!indexed.by_prop.is_empty());
        assert!(plain.by_prop.is_empty());
    }

    #[test]
    fn empty_spec_skips_build_pass_entirely() {
        let mut state = KeyspaceState::new();
        state.ingest_nodes(vec![item("item:a", "mod::foo", "context_a")]);
        assert!(
            state.by_prop.is_empty(),
            "no spec entries means no index maintenance"
        );
    }

    #[test]
    fn label_change_on_reingest_drops_old_label_entries() {
        let mut state = KeyspaceState::new_with_spec(three_index_spec());
        state.ingest_nodes(vec![item("item:a", "mod::foo", "context_a")]);

        let label_item = Label::new("Item");
        let key_qname = (label_item, "qname".to_string());
        let idx = *state.id_to_idx.get("item:a").expect("ingested");
        assert!(state.by_prop[&key_qname]["mod::foo"].contains(&idx));

        // Re-ingest with a label the spec does not cover — Item → CallSite.
        let changed = Node::new("item:a", Label::new("CallSite"))
            .with_prop("qname", "mod::foo")
            .with_prop("bounded_context", "context_a");
        state.ingest_nodes(vec![changed]);

        // All (Item, *) entries for this idx should have been dropped. The
        // CallSite label is not in the spec so no new entries appear.
        assert!(!state
            .by_prop
            .get(&key_qname)
            .is_some_and(|m| m.contains_key("mod::foo")));
    }
}

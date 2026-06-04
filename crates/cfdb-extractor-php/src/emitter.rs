//! Fact accumulator for the PHP producer.
//!
//! [`Emitter`] collects `:Node`s (deduped by id) and `:Edge`s during the
//! tree-sitter walk, buffers `implements` targets for the two-pass
//! `IMPLEMENTS` resolution (RFC-045 §3.2), and yields the fact set at
//! [`Emitter::finish`]. The [`item_id`] / [`module_id`] formatters define
//! the id space the accumulator resolves `IMPLEMENTS` targets against.

use std::collections::BTreeMap;

use cfdb_core::fact::{Edge, Node};
use cfdb_core::schema::EdgeLabel;

/// Node id for an `:Item` of fully-qualified PHP name `qname`.
pub(crate) fn item_id(qname: &str) -> String {
    format!("item:{qname}")
}

/// Node id for the `:Module` of PHP namespace `namespace`. Matches the
/// `:Item.IN_MODULE` edge target so structural containment resolves.
pub(crate) fn module_id(namespace: &str) -> String {
    format!("module:{namespace}")
}

/// Small helper that dedups `:Module` / `:Crate` node ids and otherwise
/// just collects everything for sorting at [`Emitter::finish`].
pub(crate) struct Emitter {
    nodes: BTreeMap<String, Node>,
    edges: Vec<Edge>,
    /// Buffered `(source_class_id, target_interface_qname)` pairs from
    /// pass 1, resolved to `IMPLEMENTS` edges in pass 2 once every `:Item`
    /// exists. The target may be declared in a later-walked file, so the
    /// resolution cannot happen inline (RFC-045 §3.2 two-pass).
    pending_implements: Vec<(String, String)>,
}

impl Emitter {
    pub(crate) fn new() -> Self {
        Self {
            nodes: BTreeMap::new(),
            edges: Vec::new(),
            pending_implements: Vec::new(),
        }
    }

    pub(crate) fn emit_node(&mut self, node: Node) {
        // dedup on id — if the same module/crate id is emitted twice
        // (multi-file namespace), keep the first.
        self.nodes.entry(node.id.clone()).or_insert(node);
    }

    pub(crate) fn has_node(&self, id: &str) -> bool {
        self.nodes.contains_key(id)
    }

    pub(crate) fn emit_edge(&mut self, edge: Edge) {
        self.edges.push(edge);
    }

    /// Pass 1 — buffer an `(implementing class id, target interface qname)`
    /// pair for later resolution. No edge is emitted yet.
    pub(crate) fn buffer_implements(&mut self, source_id: &str, target_qname: &str) {
        self.pending_implements
            .push((source_id.to_string(), target_qname.to_string()));
    }

    /// Pass 2 — emit an `IMPLEMENTS` edge for each buffered pair whose
    /// target interface qname resolves to an emitted in-workspace `:Item`.
    /// Unresolved targets (external `vendor/` interfaces, `use`-aliased
    /// names) are dropped: the graph never invents a placeholder node and
    /// never emits an edge that would dangle at ingest (RFC-045 §3.2 D2 /
    /// §4 I4 — stubs are not arrows). Every emitted edge carries
    /// `resolver = "tree-sitter-php"`.
    pub(crate) fn resolve_pending_implements(&mut self) {
        let pending = std::mem::take(&mut self.pending_implements);
        for (source_id, target_qname) in pending {
            let target_id = item_id(&target_qname);
            if self.nodes.contains_key(&target_id) {
                self.edges.push(
                    Edge::new(source_id, target_id, EdgeLabel::new(EdgeLabel::IMPLEMENTS))
                        .with_prop("resolver", "tree-sitter-php"),
                );
            }
        }
    }

    pub(crate) fn finish(self) -> (Vec<Node>, Vec<Edge>) {
        (self.nodes.into_values().collect(), self.edges)
    }
}

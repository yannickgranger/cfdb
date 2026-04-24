//! Shared [`Emitter`] sink — holds the accumulating nodes + edges the
//! AST walkers push into, plus the post-walk RETURNS / TYPE_OF
//! resolution state (`emitted_item_qnames` + two deferred queues per
//! RFC-037 §3.2, #216).
//!
//! Split from `lib.rs` (#239 slice) to keep the top-level module under
//! the 500-LOC architecture threshold. The public surface is
//! `pub(crate)` only — outside consumers see the extractor exclusively
//! through [`crate::extract_workspace`].

use cfdb_core::fact::{Edge, Node};

/// Shared node/edge sink. Every submodule that walks the AST holds a
/// `&mut Emitter` and pushes into these vectors; the outer
/// [`crate::extract_workspace`] owns the instance and calls
/// [`Emitter::finish`] once the workspace has been fully walked.
///
/// **RETURNS / TYPE_OF post-walk state (RFC-037 §3.2, #216; extended
/// for #239).** Three fields support deferred edge resolution:
/// `emitted_item_qnames` records every `:Item` qname the extractor has
/// emitted (populated by
/// [`crate::item_visitor::ItemVisitor::emit_item_with_flags`] and the
/// impl-method emission path); `deferred_returns` records
/// `(fn_qname, rendered_return_type_string, original_return_syn_type)`
/// tuples queued by `visit_item_fn` / `visit_impl_item_fn`; and
/// `deferred_type_of` records the same shape for `:Field` / `:Param`
/// TYPE_OF edges. Once the workspace walk is complete,
/// [`crate::resolver::resolve_deferred_returns`] /
/// [`crate::resolver::resolve_deferred_type_of`] drain the queues and
/// emit edges for every resolvable entry. Holding these on the
/// workspace-scoped `Emitter` (rather than on the per-file
/// [`crate::item_visitor::ItemVisitor`]) means the resolution loop sees
/// every item across every file regardless of walk order — a
/// `pub fn use_foo() -> Foo` declared before `pub struct Foo {}` in
/// the same file (or in a different file walked earlier) still
/// resolves correctly.
pub(crate) struct Emitter {
    nodes: Vec<Node>,
    edges: Vec<Edge>,
    /// Qnames of every `:Item` emitted so far — used for RETURNS /
    /// TYPE_OF post-walk resolution. Populated by
    /// [`crate::item_visitor::ItemVisitor::emit_item_with_flags`] and
    /// by the impl-method emission path in
    /// [`crate::item_visitor::ItemVisitor::visit_impl_item_fn`].
    pub(crate) emitted_item_qnames: std::collections::BTreeSet<String>,
    /// Deferred RETURNS edges — `(fn_item_qname,
    /// rendered_return_type_string, original_return_syn_type)`.
    /// Walked after all items are emitted. The rendered string is
    /// consulted first (exact + unique-last-segment tiers); the
    /// stored `syn::Type` powers the third-tier wrapper unwrap via
    /// [`crate::type_render::render_type_inner`] (#239), which runs
    /// only when the rendered-string tiers miss.
    pub(crate) deferred_returns: Vec<(String, String, syn::Type)>,
    /// Deferred TYPE_OF edges — `(source_node_id, rendered_type_string,
    /// source_label, original_syn_type)` where `source_label` is
    /// `"Field"` or `"Param"`. Walked in [`crate::extract_workspace`]'s
    /// Step 4 post-walk pass; emits a `TYPE_OF` edge from the source
    /// `:Field` / `:Param` node to the `:Item` whose qname matches the
    /// rendered type (exact, unique-last-segment, or `render_type_inner`
    /// wrapper unwrap on the stored `syn::Type` — #239), mirroring
    /// the RETURNS resolver. Variants are not queued from here —
    /// a variant's payload is walked into separate `:Field` nodes
    /// which queue their own TYPE_OF entries. Variant-level TYPE_OF
    /// is a documented follow-up (RFC-037 §3.4 / #220 non-goals).
    pub(crate) deferred_type_of: Vec<(String, String, &'static str, syn::Type)>,
}

impl Emitter {
    pub(crate) fn new() -> Self {
        Self {
            nodes: Vec::new(),
            edges: Vec::new(),
            emitted_item_qnames: std::collections::BTreeSet::new(),
            deferred_returns: Vec::new(),
            deferred_type_of: Vec::new(),
        }
    }

    pub(crate) fn emit_node(&mut self, node: Node) {
        self.nodes.push(node);
    }

    pub(crate) fn emit_edge(&mut self, edge: Edge) {
        self.edges.push(edge);
    }

    pub(crate) fn finish(self) -> (Vec<Node>, Vec<Edge>) {
        (self.nodes, self.edges)
    }
}

//! Fact accumulator for the PHP producer.
//!
//! [`Emitter`] collects `:Node`s (deduped by id) and `:Edge`s during the
//! tree-sitter walk, buffers `implements` targets for the two-pass
//! `IMPLEMENTS` resolution, and yields the fact set at [`Emitter::finish`].
//! The [`item_id`] / [`module_id`] formatters define the id space the
//! accumulator resolves `IMPLEMENTS` targets against.

use std::collections::BTreeMap;

use cfdb_core::fact::{Edge, Node};
use cfdb_core::schema::{EdgeLabel, Label};

/// Node id for an `:Item` of fully-qualified PHP name `qname`.
pub(crate) fn item_id(qname: &str) -> String {
    format!("item:{qname}")
}

/// Node id for the `:Module` of PHP namespace `namespace`. Matches the
/// `:Item.IN_MODULE` edge target so structural containment resolves.
pub(crate) fn module_id(namespace: &str) -> String {
    format!("module:{namespace}")
}

/// One call expression discovered during the body walk, buffered in pass 1
/// and turned into a `:CallSite` (+ `INVOKES_AT`, + `CALLS` when resolved)
/// in pass 2. The `:CallSite` node and `INVOKES_AT` edge are unconditional;
/// `callee_resolved` and the `CALLS` edge depend on whether `resolve_target`
/// names an in-workspace `:Item` — only knowable after every file is walked,
/// hence the two-pass.
pub(crate) struct PendingCallSite {
    /// `callsite:{caller_qname}:{callee_path}:{local_idx}` (the `local_idx`
    /// is the per-caller per-`callee_path` occurrence counter — matches the
    /// Rust producer's id scheme, `cfdb-extractor/src/call_visitor.rs`).
    pub id: String,
    pub caller_qname: String,
    pub callee_path: String,
    pub file: String,
    pub line: i64,
    /// In-workspace `:Item` qname this call resolves to, or `None` when the
    /// call is unresolvable in principle (dynamic dispatch, `parent::`,
    /// `$x->m()`). `Some(_)` is still only emitted as `CALLS` if the qname
    /// names an actually-emitted `:Item`.
    pub resolve_target: Option<String>,
}

/// Last path segment of a PHP `callee_path` — the part after the final `::`
/// (method separator) or `\` (namespace separator). Mirrors the Rust
/// producer's `callee_last_segment` (which splits on `::`); PHP also splits
/// on `\` so `\Ns\foo` yields `foo`, not the whole path.
pub(crate) fn callee_last_segment(callee_path: &str) -> &str {
    let after_colons = callee_path.rsplit("::").next().unwrap_or(callee_path);
    after_colons.rsplit('\\').next().unwrap_or(after_colons)
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
    /// Buffered call sites from the pass-1 body walk (RFC-045 §3.4), turned
    /// into `:CallSite` + `INVOKES_AT` (+ resolved `CALLS`) in pass 2.
    pending_call_sites: Vec<PendingCallSite>,
}

impl Emitter {
    pub(crate) fn new() -> Self {
        Self {
            nodes: BTreeMap::new(),
            edges: Vec::new(),
            pending_implements: Vec::new(),
            pending_call_sites: Vec::new(),
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
    /// never emits an edge that would dangle at ingest. Every emitted edge
    /// carries `resolver = "tree-sitter-php"`.
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

    /// Pass 1 — buffer a discovered call site for later resolution.
    pub(crate) fn buffer_call_site(&mut self, cs: PendingCallSite) {
        self.pending_call_sites.push(cs);
    }

    /// Pass 2 — for each buffered call site emit a `:CallSite` node (full
    /// Rust-parity prop set, `resolver = "tree-sitter-php"`) and the
    /// `INVOKES_AT` edge `:Item{caller} -> :CallSite`. When the
    /// `resolve_target` names an emitted in-workspace `:Item`, also set
    /// `callee_resolved = true` and emit `:Item{caller} -[:CALLS]-> :Item{callee}`.
    /// Unresolved calls carry `callee_resolved = false` and NO `CALLS`
    /// (no guessed target, never an edge that would dangle).
    pub(crate) fn resolve_pending_call_sites(&mut self) {
        let pending = std::mem::take(&mut self.pending_call_sites);
        for cs in pending {
            let callee_resolved = cs
                .resolve_target
                .as_ref()
                .is_some_and(|t| self.nodes.contains_key(&item_id(t)));

            let node = Node::new(cs.id.as_str(), Label::new(Label::CALL_SITE))
                .with_prop("caller_qname", cs.caller_qname.as_str())
                .with_prop("callee_path", cs.callee_path.as_str())
                .with_prop("callee_last_segment", callee_last_segment(&cs.callee_path))
                .with_prop("kind", "call")
                .with_prop("file", cs.file.as_str())
                .with_prop("line", cs.line)
                // PHP has no `#[cfg(test)]` analogue; test-scope detection is
                // out of MVP scope — every PHP call site is `is_test = false`.
                .with_prop("is_test", false)
                .with_prop("resolver", "tree-sitter-php")
                .with_prop("callee_resolved", callee_resolved);
            // INVOKES_AT: Item(caller) -> CallSite.
            self.edges.push(Edge::new(
                item_id(&cs.caller_qname),
                cs.id.as_str(),
                EdgeLabel::new(EdgeLabel::INVOKES_AT),
            ));

            // CALLS: Item(caller) -> Item(callee), only when resolved in-workspace.
            if callee_resolved {
                if let Some(target) = &cs.resolve_target {
                    self.edges.push(Edge::new(
                        item_id(&cs.caller_qname),
                        item_id(target),
                        EdgeLabel::new(EdgeLabel::CALLS),
                    ));
                }
            }

            // Insert the CallSite node last so the owned `cs.id` moves into the
            // map key — no per-iteration clone (the id is borrowed above for
            // the node and the INVOKES_AT edge, both of which precede this).
            self.nodes.entry(cs.id).or_insert(node);
        }
    }

    pub(crate) fn finish(self) -> (Vec<Node>, Vec<Edge>) {
        (self.nodes.into_values().collect(), self.edges)
    }
}

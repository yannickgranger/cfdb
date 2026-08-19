use std::collections::BTreeMap;

use cfdb_core::fact::{Edge, Node};
use cfdb_core::schema::{EdgeLabel, Label};

pub(crate) fn item_id(qname: &str) -> String {
    format!("item:{qname}")
}

pub(crate) fn module_id(namespace: &str) -> String {
    format!("module:{namespace}")
}

pub(crate) struct PendingCallSite {
    pub id: String,
    pub caller_qname: String,
    pub callee_path: String,
    pub file: String,
    pub line: i64,
    pub resolve_target: Option<String>,
}

pub(crate) fn callee_last_segment(callee_path: &str) -> &str {
    let after_colons = callee_path.rsplit("::").next().unwrap_or(callee_path);
    after_colons.rsplit('\\').next().unwrap_or(after_colons)
}

pub(crate) struct Emitter {
    nodes: BTreeMap<String, Node>,
    edges: Vec<Edge>,
    pending_implements: Vec<(String, String)>,
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
        self.nodes.entry(node.id.clone()).or_insert(node);
    }

    pub(crate) fn has_node(&self, id: &str) -> bool {
        self.nodes.contains_key(id)
    }

    pub(crate) fn emit_edge(&mut self, edge: Edge) {
        self.edges.push(edge);
    }

    pub(crate) fn buffer_implements(&mut self, source_id: &str, target_qname: &str) {
        self.pending_implements
            .push((source_id.to_string(), target_qname.to_string()));
    }

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

    pub(crate) fn buffer_call_site(&mut self, cs: PendingCallSite) {
        self.pending_call_sites.push(cs);
    }

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
                .with_prop("is_test", false)
                .with_prop("resolver", "tree-sitter-php")
                .with_prop("callee_resolved", callee_resolved);
            self.edges.push(Edge::new(
                item_id(&cs.caller_qname),
                cs.id.as_str(),
                EdgeLabel::new(EdgeLabel::INVOKES_AT),
            ));

            if callee_resolved {
                if let Some(target) = &cs.resolve_target {
                    self.edges.push(Edge::new(
                        item_id(&cs.caller_qname),
                        item_id(target),
                        EdgeLabel::new(EdgeLabel::CALLS),
                    ));
                }
            }

            self.nodes.entry(cs.id).or_insert(node);
        }
    }

    pub(crate) fn finish(self) -> (Vec<Node>, Vec<Edge>) {
        (self.nodes.into_values().collect(), self.edges)
    }
}

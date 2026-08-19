use cfdb_core::fact::{Edge, Node};
use cfdb_core::qname::TargetDiscriminator;

pub(crate) struct Emitter {
    nodes: Vec<Node>,
    edges: Vec<Edge>,
    pub(crate) emitted_item_qnames: std::collections::BTreeMap<String, Vec<TargetDiscriminator>>,
    pub(crate) deferred_returns: Vec<(String, TargetDiscriminator, String, syn::Type)>,
    pub(crate) deferred_type_of:
        Vec<(String, String, &'static str, syn::Type, TargetDiscriminator)>,
    pub(crate) deferred_match_targets: Vec<(String, String, TargetDiscriminator)>,
    pub(crate) emitted_enum_qnames: std::collections::BTreeMap<String, Vec<TargetDiscriminator>>,
}

impl Emitter {
    pub(crate) fn new() -> Self {
        Self {
            nodes: Vec::new(),
            edges: Vec::new(),
            emitted_item_qnames: std::collections::BTreeMap::new(),
            deferred_returns: Vec::new(),
            deferred_type_of: Vec::new(),
            deferred_match_targets: Vec::new(),
            emitted_enum_qnames: std::collections::BTreeMap::new(),
        }
    }

    pub(crate) fn claim_item_qname(&mut self, qname: &str, target: &TargetDiscriminator) {
        let claims = self
            .emitted_item_qnames
            .entry(qname.to_string())
            .or_default();
        if !claims.contains(target) {
            claims.push(target.clone());
        }
    }

    pub(crate) fn claim_enum_qname(&mut self, qname: &str, target: &TargetDiscriminator) {
        let claims = self
            .emitted_enum_qnames
            .entry(qname.to_string())
            .or_default();
        if !claims.contains(target) {
            claims.push(target.clone());
        }
    }

    pub(crate) fn emit_node(&mut self, node: Node) {
        self.nodes.push(node);
    }

    pub(crate) fn emit_edge(&mut self, edge: Edge) {
        self.edges.push(edge);
    }

    pub(crate) fn edges(&self) -> &[Edge] {
        &self.edges
    }

    pub(crate) fn finish(self) -> (Vec<Node>, Vec<Edge>) {
        (self.nodes, self.edges)
    }
}

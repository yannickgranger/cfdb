use std::collections::BTreeSet;

use cfdb_core::fact::{Edge, Node};
use cfdb_core::qname::display_qname_from_node_id;
use cfdb_core::query::item_kind::ItemKind;
use cfdb_core::schema::{EdgeLabel, Keyspace, Label};
use cfdb_core::store::{StoreBackend, StoreError};
use cfdb_hir_extractor::emit::{CallSiteEmitter, EmitStats};
use cfdb_petgraph::PetgraphStore;

pub struct PetgraphAdapter<'s> {
    store: &'s mut PetgraphStore,
    keyspace: Keyspace,
}

impl<'s> PetgraphAdapter<'s> {
    #[must_use]
    pub fn new(store: &'s mut PetgraphStore, keyspace: Keyspace) -> Self {
        Self { store, keyspace }
    }
}

impl CallSiteEmitter for PetgraphAdapter<'_> {
    type Err = StoreError;

    fn ingest_resolved_call_sites(
        &mut self,
        mut nodes: Vec<Node>,
        edges: Vec<Edge>,
    ) -> Result<EmitStats, Self::Err> {
        let call_sites_emitted = nodes
            .iter()
            .filter(|n| n.label.as_str() == Label::CALL_SITE)
            .count();
        let entry_points_emitted = nodes
            .iter()
            .filter(|n| n.label.as_str() == Label::ENTRY_POINT)
            .count();
        let calls_edges_emitted = edges
            .iter()
            .filter(|e| e.label.as_str() == EdgeLabel::CALLS)
            .count();
        let invokes_at_edges_emitted = edges
            .iter()
            .filter(|e| e.label.as_str() == EdgeLabel::INVOKES_AT)
            .count();
        let exposes_edges_emitted = edges
            .iter()
            .filter(|e| e.label.as_str() == EdgeLabel::EXPOSES)
            .count();

        synthesize_callee_stubs(&edges, &mut nodes, self.store, &self.keyspace);

        self.store.ingest_nodes(&self.keyspace, nodes)?;
        self.store.ingest_edges(&self.keyspace, edges)?;

        Ok(EmitStats {
            call_sites_emitted,
            calls_edges_emitted,
            invokes_at_edges_emitted,
            entry_points_emitted,
            exposes_edges_emitted,
        })
    }
}

fn synthesize_callee_stubs(
    edges: &[Edge],
    nodes: &mut Vec<Node>,
    store: &PetgraphStore,
    keyspace: &Keyspace,
) {
    let pending_ids: BTreeSet<&str> = nodes
        .iter()
        .filter(|n| n.label.as_str() == Label::ITEM)
        .map(|n| n.id.as_str())
        .collect();
    let mut missing: BTreeSet<&str> = BTreeSet::new();
    for edge in edges {
        if edge.label.as_str() != EdgeLabel::CALLS {
            continue;
        }
        let dst = edge.dst.as_str();
        if pending_ids.contains(dst) {
            continue;
        }
        if store.has_node(keyspace, dst) {
            continue;
        }
        missing.insert(dst);
    }
    for dst_id in missing {
        nodes.push(build_callee_stub(dst_id));
    }
}

fn build_callee_stub(node_id: &str) -> Node {
    let qname = display_qname_from_node_id(node_id);
    let crate_name = qname
        .split_once("::")
        .map(|(c, _)| c.to_string())
        .unwrap_or_else(|| qname.to_string());

    let props = cfdb_core::fact::build_item_props(
        qname,
        ItemKind::Fn.to_extractor_str(),
        &crate_name,
        &crate_name,
    );

    Node {
        id: node_id.to_string(),
        label: Label::new(Label::ITEM),
        props,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cfdb_core::fact::PropValue;
    use cfdb_core::qname::item_node_id;
    use std::collections::BTreeMap;

    fn hir_call_site(id: &str, caller_qname: &str, callee_path: &str) -> Node {
        let mut props = BTreeMap::new();
        props.insert(
            "caller_qname".into(),
            PropValue::Str(caller_qname.to_string()),
        );
        props.insert(
            "callee_path".into(),
            PropValue::Str(callee_path.to_string()),
        );
        props.insert("resolver".into(), PropValue::Str("hir".to_string()));
        props.insert("callee_resolved".into(), PropValue::Bool(true));
        Node {
            id: id.to_string(),
            label: Label::new(Label::CALL_SITE),
            props,
        }
    }

    fn edge(src: &str, dst: &str, label: &str) -> Edge {
        Edge {
            src: src.to_string(),
            dst: dst.to_string(),
            label: EdgeLabel::new(label),
            props: BTreeMap::new(),
        }
    }

    fn keyspace() -> Keyspace {
        Keyspace::new("test")
    }

    #[test]
    fn ingest_counts_call_sites_calls_and_invokes_at() {
        let mut store = PetgraphStore::new();
        let mut adapter = PetgraphAdapter::new(&mut store, keyspace());

        let nodes = vec![
            hir_call_site("callsite:a::foo:bar:0", "a::foo", "bar"),
            hir_call_site("callsite:a::foo:baz:0", "a::foo", "baz"),
        ];
        let edges = vec![
            edge("item:a::foo", "item:a::bar", EdgeLabel::CALLS),
            edge("item:a::foo", "item:a::baz", EdgeLabel::CALLS),
            edge(
                "item:a::foo",
                "callsite:a::foo:bar:0",
                EdgeLabel::INVOKES_AT,
            ),
            edge(
                "item:a::foo",
                "callsite:a::foo:baz:0",
                EdgeLabel::INVOKES_AT,
            ),
        ];

        let stats = adapter
            .ingest_resolved_call_sites(nodes, edges)
            .expect("ingest succeeds on fresh store");

        assert_eq!(stats.call_sites_emitted, 2);
        assert_eq!(stats.calls_edges_emitted, 2);
        assert_eq!(stats.invokes_at_edges_emitted, 2);
    }

    #[test]
    fn ingest_with_empty_batches_returns_zero_stats() {
        let mut store = PetgraphStore::new();
        let mut adapter = PetgraphAdapter::new(&mut store, keyspace());

        let stats = adapter
            .ingest_resolved_call_sites(Vec::new(), Vec::new())
            .expect("empty ingest succeeds");

        assert_eq!(stats, EmitStats::default());
    }

    fn item_node(qname: &str, crate_name: &str) -> Node {
        let mut props = cfdb_core::fact::build_item_props(qname, "fn", crate_name, crate_name);
        props.insert(
            "file".into(),
            cfdb_core::fact::PropValue::Str("src/lib.rs".to_string()),
        );
        Node {
            id: item_node_id(qname),
            label: Label::new(Label::ITEM),
            props,
        }
    }

    #[test]
    fn ingest_synthesizes_stub_item_for_unknown_calls_dst() {
        let mut store = PetgraphStore::new();
        store
            .ingest_nodes(&keyspace(), vec![item_node("a::foo", "a")])
            .expect("syn-side caller ingest succeeds");

        let mut adapter = PetgraphAdapter::new(&mut store, keyspace());

        let nodes = vec![hir_call_site(
            "callsite:a::foo:std::vec::Vec::push:0",
            "a::foo",
            "std::vec::Vec::push",
        )];
        let edges = vec![
            edge("item:a::foo", "item:std::vec::Vec::push", EdgeLabel::CALLS),
            edge(
                "item:a::foo",
                "callsite:a::foo:std::vec::Vec::push:0",
                EdgeLabel::INVOKES_AT,
            ),
        ];

        adapter
            .ingest_resolved_call_sites(nodes, edges)
            .expect("ingest with stub synthesis succeeds");

        assert!(
            store.has_node(&keyspace(), "item:std::vec::Vec::push"),
            "stub :Item for foreign callee `std::vec::Vec::push` must be \
             synthesized so the CALLS edge survives ingest (#388)",
        );

        let (_, edges_in_store) = store.export(&keyspace()).expect("export");
        let calls_landed = edges_in_store
            .iter()
            .any(|e| e.label.as_str() == EdgeLabel::CALLS && e.dst == "item:std::vec::Vec::push");
        assert!(
            calls_landed,
            "CALLS edge to synthesized stub must survive ingest"
        );
    }

    #[test]
    fn ingest_does_not_clobber_existing_calls_dst_item() {
        let mut store = PetgraphStore::new();
        store
            .ingest_nodes(&keyspace(), vec![item_node("a::callee", "a")])
            .expect("syn-side ingest succeeds");

        let mut adapter = PetgraphAdapter::new(&mut store, keyspace());

        let nodes = vec![hir_call_site(
            "callsite:a::caller:a::callee:0",
            "a::caller",
            "a::callee",
        )];
        let edges = vec![
            edge("item:a::caller", "item:a::callee", EdgeLabel::CALLS),
            edge(
                "item:a::caller",
                "callsite:a::caller:a::callee:0",
                EdgeLabel::INVOKES_AT,
            ),
        ];

        adapter
            .ingest_resolved_call_sites(nodes, edges)
            .expect("ingest with pre-existing dst succeeds");

        let (nodes_after, _) = store.export(&keyspace()).expect("export");
        let callee = nodes_after
            .iter()
            .find(|n| n.id == "item:a::callee")
            .expect("pre-existing :Item still present");
        assert_eq!(
            callee.props.get("file").and_then(PropValue::as_str),
            Some("src/lib.rs"),
            "pre-existing :Item's body-shaped `file` prop must NOT be \
             clobbered by stub synthesis (#388 idempotency invariant)",
        );
    }

    #[test]
    fn ingest_synthesizes_stub_under_discriminated_dst_id_verbatim() {
        let mut store = PetgraphStore::new();
        store
            .ingest_nodes(&keyspace(), vec![item_node("a::foo", "a")])
            .expect("caller ingest succeeds");
        let mut adapter = PetgraphAdapter::new(&mut store, keyspace());

        let nodes = vec![hir_call_site(
            "callsite:a::foo:b::helper:0",
            "a::foo",
            "b::helper",
        )];
        let edges = vec![
            edge("item:a::foo", "item:b::helper#bin:tool", EdgeLabel::CALLS),
            edge(
                "item:a::foo",
                "callsite:a::foo:b::helper:0",
                EdgeLabel::INVOKES_AT,
            ),
        ];
        adapter
            .ingest_resolved_call_sites(nodes, edges)
            .expect("ingest with discriminated dst succeeds");

        assert!(
            store.has_node(&keyspace(), "item:b::helper#bin:tool"),
            "stub must land under the discriminated dst id VERBATIM, \
             or the CALLS edge it anchors still dangles (54-C)"
        );
        let (nodes_after, _) = store.export(&keyspace()).expect("export");
        let stub = nodes_after
            .iter()
            .find(|n| n.id == "item:b::helper#bin:tool")
            .expect("discriminated stub present");
        assert_eq!(
            stub.props.get("qname").and_then(PropValue::as_str),
            Some("b::helper"),
            "stub display qname must be the BARE qname (suffix stripped)"
        );
    }

    #[test]
    fn ingest_does_not_count_non_hir_edge_labels() {
        let mut store = PetgraphStore::new();
        let mut adapter = PetgraphAdapter::new(&mut store, keyspace());

        let nodes = vec![hir_call_site("callsite:a:b:0", "a", "b")];
        let edges = vec![
            edge("item:a", "crate:a", EdgeLabel::IN_CRATE),
            edge("item:a", "callsite:a:b:0", EdgeLabel::INVOKES_AT),
        ];

        let stats = adapter
            .ingest_resolved_call_sites(nodes, edges)
            .expect("mixed batch ingests");

        assert_eq!(stats.call_sites_emitted, 1);
        assert_eq!(
            stats.calls_edges_emitted, 0,
            "IN_CRATE is not a HIR CALLS edge and must not be counted"
        );
        assert_eq!(stats.invokes_at_edges_emitted, 1);
    }
}

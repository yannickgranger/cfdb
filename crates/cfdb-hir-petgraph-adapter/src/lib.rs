//! `cfdb-hir-petgraph-adapter` — orphan-rule bridge between
//! `cfdb-hir-extractor`'s `CallSiteEmitter` trait and
//! `cfdb-petgraph`'s `PetgraphStore` type.
//!
//! ## Why a dedicated adapter crate (not an impl in `cfdb-petgraph`)
//!
//! RFC-032 §3 lines 221–227 require that `cfdb-cli` NOT pay the
//! 90–150s `ra-ap-*` cold-compile cost on every build. Because
//! `cfdb-cli → cfdb-petgraph` already exists, placing
//! `impl CallSiteEmitter for PetgraphStore` INSIDE `cfdb-petgraph`
//! would transitively pull `ra-ap-*` into `cfdb-cli`'s compile tree
//! on every default build (via `cfdb-petgraph → cfdb-hir-extractor
//! → ra-ap-*`). Architect review of #40 flagged this as a CRITICAL
//! rust-systems finding.
//!
//! The fix is this crate: the impl lives here, and `cfdb-cli` never
//! depends on it directly. Slice 4 (Issue #86) adds the `hir`
//! feature flag on `cfdb-cli` that OPTIONALLY pulls this adapter —
//! users who do not enable the feature pay zero HIR compile cost.
//!
//! ## Orphan-rule justification
//!
//! - Trait `CallSiteEmitter` lives in `cfdb-hir-extractor`.
//! - Target type `PetgraphStore` lives in `cfdb-petgraph`.
//! - Implementation lives HERE. Valid because this crate depends on
//!   both the trait source and the target type — the orphan rule
//!   forbids impls only when NEITHER is local, which is not our
//!   case.
//!
//! ## Slice status — Issue #92
//!
//! This slice ships the adapter + architecture test. The
//! `extract_call_sites<DB: HirDatabase + Sized>` free function that
//! produces the `(Vec<Node>, Vec<Edge>)` inputs is deferred to slice
//! 3c (Issue #85c). Until then, callers construct the input manually
//! (as the unit test below does) — useful for exercising the store's
//! ingestion path without requiring a loaded `HirDatabase`.

use cfdb_core::fact::{Edge, Node};
use cfdb_core::schema::{EdgeLabel, Keyspace, Label};
use cfdb_core::store::{StoreBackend, StoreError};
use cfdb_hir_extractor::emit::{CallSiteEmitter, EmitStats};
use cfdb_petgraph::PetgraphStore;

/// Adapter that pairs a mutable borrow of a [`PetgraphStore`] with a
/// target [`Keyspace`]. `CallSiteEmitter` is implemented on the
/// adapter rather than directly on `PetgraphStore` because the store
/// ingestion API is keyspace-parameterized — the adapter threads the
/// keyspace through without bleeding it into the trait signature.
pub struct PetgraphAdapter<'s> {
    store: &'s mut PetgraphStore,
    keyspace: Keyspace,
}

impl<'s> PetgraphAdapter<'s> {
    /// Construct an adapter targeting `keyspace` in `store`. The
    /// keyspace is created lazily on first ingest if it does not yet
    /// exist (matching `StoreBackend::ingest_nodes` semantics).
    #[must_use]
    pub fn new(store: &'s mut PetgraphStore, keyspace: Keyspace) -> Self {
        Self { store, keyspace }
    }
}

impl CallSiteEmitter for PetgraphAdapter<'_> {
    type Err = StoreError;

    fn ingest_resolved_call_sites(
        &mut self,
        nodes: Vec<Node>,
        edges: Vec<Edge>,
    ) -> Result<EmitStats, Self::Err> {
        let call_sites_emitted = nodes
            .iter()
            .filter(|n| n.label.as_str() == Label::CALL_SITE)
            .count();
        let calls_edges_emitted = edges
            .iter()
            .filter(|e| e.label.as_str() == EdgeLabel::CALLS)
            .count();
        let invokes_at_edges_emitted = edges
            .iter()
            .filter(|e| e.label.as_str() == EdgeLabel::INVOKES_AT)
            .count();

        self.store.ingest_nodes(&self.keyspace, nodes)?;
        self.store.ingest_edges(&self.keyspace, edges)?;

        Ok(EmitStats {
            call_sites_emitted,
            calls_edges_emitted,
            invokes_at_edges_emitted,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cfdb_core::fact::PropValue;
    use std::collections::BTreeMap;

    /// Helper: minimal `:CallSite` node fixture with the v0.1.3
    /// discriminator props (resolver/callee_resolved) set for the
    /// HIR-resolved case.
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

    #[test]
    fn ingest_does_not_count_non_hir_edge_labels() {
        let mut store = PetgraphStore::new();
        let mut adapter = PetgraphAdapter::new(&mut store, keyspace());

        // IN_CRATE is a structural edge emitted by cfdb-extractor,
        // not the HIR extractor. The adapter's stats must not count
        // it even though it lands in the store.
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

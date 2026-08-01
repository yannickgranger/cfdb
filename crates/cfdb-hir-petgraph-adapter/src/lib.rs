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

use std::collections::BTreeSet;

use cfdb_core::fact::{Edge, Node};
use cfdb_core::qname::display_qname_from_node_id;
use cfdb_core::query::item_kind::ItemKind;
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

        // Issue #388 — synthesize stub `:Item` nodes for CALLS dsts
        // that don't yet have one. Without this, every HIR-resolved
        // call to a foreign-crate callee (`std::vec::Vec::push`,
        // `serde::Serialize::serialize`, …) is silently dropped by
        // `cfdb_petgraph::graph::ingest_one_edge` ("unknown dst id ⇒
        // skip"), collapsing call-graph recall by ~99.5% on cfdb-self
        // observed at run 524. Symmetric to
        // `cfdb-extractor::synthesize::synthesize_referenced_items`
        // which handles the IMPLEMENTS/IMPLEMENTS_FOR/RETURNS/TYPE_OF
        // family on the syn side; this is the parallel for CALLS on
        // the HIR side.
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

/// Append a minimal stub `:Item` node to `nodes` for every CALLS
/// edge dst id that is NOT already covered by either:
///
/// 1. A `:Item` node in the to-be-ingested `nodes` vec (rare for
///    this adapter — HIR emits :CallSite + :EntryPoint, not :Item —
///    but the check is cheap and future-proofs against new HIR-side
///    :Item emissions).
/// 2. An existing `:Item` in the store from a prior syn-side ingest
///    (`PetgraphStore::has_node`, issue #388).
///
/// The stub carries only identity props (`qname`, `name`, `kind`,
/// `crate`, `bounded_context`). Body-shaped props (`file`, `line`,
/// `visibility`, `signature`, …) are deliberately omitted — their
/// absence is the discriminator between "walked from source" and
/// "referenced only" (RFC-039 withdrawal rationale; same convention
/// used by `cfdb-extractor::synthesize::synthesize_referenced_items`).
///
/// `bounded_context` defaults to the crate name (first `::` segment
/// of the qname). For HIR-resolved foreign callees this matches the
/// `cfdb_concepts::compute_bounded_context` heuristic with empty
/// overrides — the same value the syn-side synthesizer would write
/// for a foreign reference. For an in-workspace callee that somehow
/// missed syn extraction (conditional compilation, ...) the value
/// may diverge from the workspace's override-configured context;
/// acceptable degradation since this path should be rare.
fn synthesize_callee_stubs(
    edges: &[Edge],
    nodes: &mut Vec<Node>,
    store: &PetgraphStore,
    keyspace: &Keyspace,
) {
    // Collect distinct CALLS dst ids needing synthesis. BTreeSet for
    // deterministic stub order — appending to `nodes` in sorted-id
    // order keeps the post-ingest by_label posting list stable
    // across runs (G1 invariant).
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

/// Build a stub `:Item` node from a node id of the form
/// `item:<qname>[#bin:<name>]`. Mirrors the prop shape produced by
/// `cfdb-extractor::synthesize::synthesize_referenced_items` for
/// the IMPLEMENTS/RETURNS/TYPE_OF family, with `kind = "fn"`
/// (the most general callable value in `ItemKind`). Routes through
/// `cfdb_core::fact::build_item_props` so the prop vocabulary is
/// single-sourced (#421 boy-scout: closes the split-brain flagged by
/// `audit-split-brain` against `cfdb-extractor::synthesize`).
///
/// RFC-054 54-C: the stub's node id is the edge dst VERBATIM — a
/// discriminated dst (`item:x::y#bin:z`) must not be re-wrapped from
/// its stripped display qname, or the stub lands under a DIFFERENT id
/// and the CALLS edge it exists to anchor still dangles. The stripped
/// qname feeds the display props only (RFC-054 §3.5.1).
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

    /// Build an `:Item` node fixture so a test can pre-populate the
    /// store with a "syn-side already wrote this item" baseline. Uses
    /// the canonical `cfdb_core::fact::build_item_props` helper (single
    /// source of truth for the 5-key Item.props shape) and tacks on the
    /// `file` prop used by the test to prove the stub synthesizer
    /// didn't clobber a pre-existing :Item.
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

    /// Issue #388 — CALLS edge whose dst has no `:Item` triggers
    /// stub synthesis. The stub carries identity props (qname, name,
    /// kind=fn, crate, bounded_context) so the edge survives ingest.
    #[test]
    fn ingest_synthesizes_stub_item_for_unknown_calls_dst() {
        let mut store = PetgraphStore::new();
        // Pre-populate the caller `:Item` (would normally come from
        // the syn-side walk). The stub-synthesis test focuses on
        // the dst side; src-side recovery would be a separate
        // concern.
        store
            .ingest_nodes(&keyspace(), vec![item_node("a::foo", "a")])
            .expect("syn-side caller ingest succeeds");

        let mut adapter = PetgraphAdapter::new(&mut store, keyspace());

        // The CallSite's caller_qname references a foreign callee
        // that has no :Item in the store yet.
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

        // The stub must exist in the store under the synthesized id.
        assert!(
            store.has_node(&keyspace(), "item:std::vec::Vec::push"),
            "stub :Item for foreign callee `std::vec::Vec::push` must be \
             synthesized so the CALLS edge survives ingest (#388)",
        );

        // The CALLS edge must now be present (would have been
        // dropped without the stub).
        let (_, edges_in_store) = store.export(&keyspace()).expect("export");
        let calls_landed = edges_in_store
            .iter()
            .any(|e| e.label.as_str() == EdgeLabel::CALLS && e.dst == "item:std::vec::Vec::push");
        assert!(
            calls_landed,
            "CALLS edge to synthesized stub must survive ingest"
        );
    }

    /// Issue #388 — when the CALLS dst :Item is ALREADY in the
    /// store (from a prior syn-side ingest), the synthesizer must
    /// NOT clobber it. Verify by reading back the pre-existing
    /// body-shaped `file` prop after the HIR ingest.
    #[test]
    fn ingest_does_not_clobber_existing_calls_dst_item() {
        let mut store = PetgraphStore::new();
        // Pre-populate the store with an :Item that has a
        // body-shaped `file` prop. Simulates the syn-side walk
        // having already emitted this fn's full :Item.
        store
            .ingest_nodes(&keyspace(), vec![item_node("a::callee", "a")])
            .expect("syn-side ingest succeeds");

        let mut adapter = PetgraphAdapter::new(&mut store, keyspace());

        // HIR side now ingests a CALLS edge whose dst is the
        // existing :Item. No stub should be synthesized.
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

        // Read back the :Item and assert the body-shaped `file`
        // prop is intact — the stub synthesizer must NOT have
        // re-emitted a body-less stub that clobbered it.
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

    /// RFC-054 54-C: a DISCRIMINATED CALLS dst (`item:x#bin:y`) with no
    /// pre-existing :Item must get a stub whose node id is the dst id
    /// VERBATIM — re-wrapping the stripped display qname would land the
    /// stub under `item:x` and the edge would still dangle. Display
    /// props stay bare (RFC-054 §3.5.1).
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

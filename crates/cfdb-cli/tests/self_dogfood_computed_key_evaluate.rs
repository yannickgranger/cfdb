//! Self-dogfood test for RFC-035 slice 3 (#182).
//!
//! Extracts cfdb's own worktree and asserts that, for every `:Item`
//! node carrying a `qname` prop, the two canonical `last_segment`
//! entry points agree byte-for-byte:
//!
//! - [`ComputedKey::LastSegment.evaluate(qname)`][eval] — the
//!   compile-time dispatch surface consumed by the index-build pass.
//! - [`cfdb_core::qname::last_segment(qname)`][helper] — the direct
//!   invariant-owner call.
//!
//! Divergence would mean the dispatch path bypasses the canonical
//! owner (the exact split-brain RFC-035 §3.3 / R1 B3 is designed to
//! prevent). This test is the "Self dogfood (cfdb on cfdb)" row of
//! the Tests template for issue #182 (cfdb CLAUDE.md §2.5).
//!
//! [eval]: cfdb_petgraph::index::ComputedKey::evaluate
//! [helper]: cfdb_core::qname::last_segment

use std::path::PathBuf;

use cfdb_core::fact::PropValue;
use cfdb_core::schema::{Keyspace, Label};
use cfdb_core::store::StoreBackend;
use cfdb_petgraph::index::ComputedKey;
use cfdb_petgraph::PetgraphStore;

fn cfdb_workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("CARGO_MANIFEST_DIR must have two parents")
        .to_path_buf()
}

#[test]
fn computed_key_evaluate_matches_qname_helper_on_every_item() {
    let workspace = cfdb_workspace_root();
    let (nodes, edges) = cfdb_extractor::extract_workspace(&workspace).expect("extract cfdb");

    let mut store = PetgraphStore::new().with_workspace(&workspace);
    let ks = Keyspace::new("slice3_selfdog");
    store.ingest_nodes(&ks, nodes).expect("ingest nodes");
    store.ingest_edges(&ks, edges).expect("ingest edges");

    let (all_nodes, _) = store.export(&ks).expect("export");
    let items: Vec<_> = all_nodes
        .iter()
        .filter(|n| n.label.as_str() == Label::ITEM)
        .collect();
    assert!(!items.is_empty(), "cfdb extract produced zero :Item nodes");

    // Every :Item MUST carry a qname prop — it is the id backbone per
    // cfdb-core::qname's module doc. If the extractor ever emits an
    // :Item without one, this test surfaces the regression.
    let with_qname: Vec<(&str, &str)> = items
        .iter()
        .filter_map(|n| {
            n.props
                .get("qname")
                .and_then(PropValue::as_str)
                .map(|q| (n.id.as_str(), q))
        })
        .collect();
    assert_eq!(
        with_qname.len(),
        items.len(),
        "{} :Item nodes lack a qname prop (extractor invariant violation)",
        items.len() - with_qname.len(),
    );

    for (id, qname) in &with_qname {
        let via_dispatch = ComputedKey::LastSegment.evaluate(qname);
        let via_helper = cfdb_core::qname::last_segment(qname);
        assert_eq!(
            via_dispatch, via_helper,
            "RFC-035 §3.3 invariant violation on :Item {id:?} (qname {qname:?}):\n  \
             ComputedKey::LastSegment.evaluate → {via_dispatch:?}\n  \
             cfdb_core::qname::last_segment → {via_helper:?}\n\
             The dispatch surface must delegate to the canonical helper byte-for-byte."
        );
    }
}

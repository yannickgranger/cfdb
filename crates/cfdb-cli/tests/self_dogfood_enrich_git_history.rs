#![cfg(feature = "git-enrich")]

use std::path::PathBuf;

use cfdb_core::enrich::EnrichBackend;
use cfdb_core::fact::PropValue;
use cfdb_core::schema::{Keyspace, Label};
use cfdb_core::store::StoreBackend;
use cfdb_enrich::EnrichEngine;
use cfdb_petgraph::PetgraphStore;

fn cfdb_workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("CARGO_MANIFEST_DIR must have two parents")
        .to_path_buf()
}

#[test]
fn ac4_ac5_self_dogfood_eighty_percent_items_have_git_attrs() {
    let workspace = cfdb_workspace_root();

    let (nodes, edges) =
        cfdb_extractor::extract_workspace(&workspace).expect("extract cfdb workspace");

    let mut store = PetgraphStore::new().with_workspace(&workspace);
    let ks = Keyspace::new("selfdog");
    store
        .ingest_nodes(&ks, nodes)
        .expect("ingest extractor nodes");
    store
        .ingest_edges(&ks, edges)
        .expect("ingest extractor edges");

    let report = EnrichEngine::new(&mut store)
        .enrich_git_history(&ks)
        .expect("enrich_git_history");
    assert!(
        report.ran,
        "enrich_git_history must actually run on a git-tracked workspace: {:?}",
        report.warnings
    );

    let (all_nodes, _) = store.export(&ks).expect("export");
    let item_count = all_nodes
        .iter()
        .filter(|n| n.label.as_str() == Label::ITEM)
        .count();
    assert!(
        item_count > 0,
        "cfdb extract produced zero :Item nodes — extractor regression or wrong workspace"
    );
    let expected_min_attrs = u64::try_from(item_count).unwrap_or(u64::MAX);
    assert!(
        report.attrs_written >= expected_min_attrs,
        "AC-4: attrs_written ({}) must be ≥ item_count ({})",
        report.attrs_written,
        item_count
    );

    let with_ts = all_nodes
        .iter()
        .filter(|n| n.label.as_str() == Label::ITEM)
        .filter(|n| {
            matches!(
                n.props.get("git_last_commit_unix_ts"),
                Some(PropValue::Int(_))
            )
        })
        .count();
    let ratio = (with_ts as f64) / (item_count as f64);
    assert!(
        ratio >= 0.80,
        "AC-5: {:.1}% of :Item nodes have non-null git_last_commit_unix_ts \
         ({} of {}) — must be ≥ 80%. Either the extractor is emitting items \
         for files not tracked in git, or git-history collection missed paths.",
        ratio * 100.0,
        with_ts,
        item_count
    );

    eprintln!(
        "self-dogfood: {with_ts}/{item_count} :Item nodes ({:.1}%) have non-null git_last_commit_unix_ts",
        ratio * 100.0
    );
}

use std::path::PathBuf;

use cfdb_core::schema::Keyspace;
use cfdb_hir_extractor::emit::CallSiteEmitter;
use cfdb_hir_extractor::{build_hir_database, extract_call_sites};
use cfdb_hir_petgraph_adapter::PetgraphAdapter;
use cfdb_petgraph::PetgraphStore;

fn cfdb_workspace_root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut cur = manifest.as_path();
    while let Some(parent) = cur.parent() {
        let cargo = parent.join("Cargo.toml");
        if cargo.is_file() {
            if let Ok(contents) = std::fs::read_to_string(&cargo) {
                if contents.contains("[workspace]") {
                    return parent.to_path_buf();
                }
            }
        }
        cur = parent;
    }
    panic!(
        "could not find cfdb workspace root above {}",
        manifest.display()
    );
}

#[test]
#[ignore = "self-dogfood loads cfdb's full workspace into RootDatabase — tens of seconds; run explicitly"]
fn cfdb_self_dogfood_emits_at_least_one_resolved_call_site() {
    let root = cfdb_workspace_root();

    let (db, vfs, _pm_client, targets) =
        build_hir_database(&root, false).expect("build_hir_database on cfdb's own workspace root");

    let (nodes, edges) = extract_call_sites(&db, &vfs, &root, &targets)
        .expect("extract_call_sites on cfdb's own workspace");

    let mut store = PetgraphStore::new();
    let mut adapter = PetgraphAdapter::new(&mut store, Keyspace::new("cfdb-hir"));

    let stats = adapter
        .ingest_resolved_call_sites(nodes, edges)
        .expect("adapter ingestion on cfdb's own facts");

    assert!(
        stats.call_sites_emitted >= 1,
        "self-dogfood expected ≥1 resolved :CallSite from cfdb's own tree; got {:?}",
        stats,
    );
    assert!(
        stats.invokes_at_edges_emitted >= 1,
        "self-dogfood expected ≥1 INVOKES_AT edge from cfdb's own tree; got {:?}",
        stats,
    );
    assert!(
        stats.calls_edges_emitted >= 1,
        "self-dogfood expected ≥1 CALLS edge from cfdb's own tree; got {:?}",
        stats,
    );

    eprintln!(
        "self-dogfood stats on cfdb tree: call_sites={}, calls={}, invokes_at={}",
        stats.call_sites_emitted, stats.calls_edges_emitted, stats.invokes_at_edges_emitted,
    );
}

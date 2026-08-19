use std::fs;
use std::path::Path;

use cfdb_core::result::WarningKind;
use cfdb_core::schema::Keyspace;
use cfdb_core::store::StoreBackend;
use cfdb_hir_extractor::emit::CallSiteEmitter;
use cfdb_hir_extractor::{build_hir_database, extract_call_sites};
use cfdb_hir_petgraph_adapter::PetgraphAdapter;
use cfdb_petgraph::PetgraphStore;
use tempfile::tempdir;

fn write(root: &Path, rel: &str, contents: &str) {
    let p = root.join(rel);
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent).expect("fixture mkdir -p");
    }
    fs::write(p, contents).expect("fixture write");
}

#[test]
fn converging_syn_and_hir_call_site_overlays_silently() {
    let tmp = tempdir().expect("tempdir");
    let root = tmp.path();
    write(
        root,
        "Cargo.toml",
        "[workspace]\nresolver = \"2\"\nmembers = [\"a\", \"b\"]\n",
    );
    write(
        root,
        "a/Cargo.toml",
        r#"[package]
name = "a"
version = "0.0.1"
edition = "2021"

[dependencies]
b = { path = "../b" }
"#,
    );
    write(
        root,
        "a/src/lib.rs",
        "pub fn caller() -> i32 {\n    b::helper()\n}\n",
    );
    write(
        root,
        "b/Cargo.toml",
        r#"[package]
name = "b"
version = "0.0.1"
edition = "2021"
"#,
    );
    write(root, "b/src/lib.rs", "pub fn helper() -> i32 {\n    3\n}\n");

    let ks = Keyspace::new("overlay");
    let mut store = PetgraphStore::new();

    let (syn_nodes, syn_edges) =
        cfdb_extractor::extract_workspace(root).expect("syn extract_workspace on overlay fixture");
    let converging_id = "callsite:a::caller:b::helper:0";
    assert!(
        syn_nodes.iter().any(|n| n.id == converging_id),
        "fixture must produce the converging syn :CallSite `{converging_id}` — got: {:?}",
        syn_nodes
            .iter()
            .filter(|n| n.id.starts_with("callsite:"))
            .map(|n| n.id.as_str())
            .collect::<Vec<_>>()
    );
    store
        .ingest_nodes(&ks, syn_nodes)
        .expect("syn node ingest succeeds");
    store
        .ingest_edges(&ks, syn_edges)
        .expect("syn edge ingest succeeds");

    let (db, vfs, _pm, targets) =
        build_hir_database(root, false).expect("build_hir_database on overlay fixture");
    let (hir_nodes, hir_edges) =
        extract_call_sites(&db, &vfs, root, &targets).expect("extract_call_sites on overlay");
    assert!(
        hir_nodes.iter().any(|n| n.id == converging_id),
        "HIR must resolve the same converging :CallSite `{converging_id}` — got: {:?}",
        hir_nodes.iter().map(|n| n.id.as_str()).collect::<Vec<_>>()
    );
    let mut adapter = PetgraphAdapter::new(&mut store, ks.clone());
    adapter
        .ingest_resolved_call_sites(hir_nodes, hir_edges)
        .expect("HIR overlay ingest succeeds");

    let contentions: Vec<_> = store
        .ingest_warnings(&ks)
        .iter()
        .filter(|w| w.kind == WarningKind::IdentityContention)
        .cloned()
        .collect();
    assert!(
        contentions.is_empty(),
        "#561: a converging syn/HIR call site must overlay silently \
         (same workspace-relative file) — got contention warnings: {contentions:?}"
    );
}

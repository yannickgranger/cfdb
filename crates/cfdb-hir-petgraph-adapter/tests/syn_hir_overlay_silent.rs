//! #561 (RFC-054 54-A known limitation, closed by 54-C's companion):
//! when a syn `:CallSite` and an HIR `:CallSite` CONVERGE on one id
//! (textual callee path == resolved path, same caller identity, same
//! local index), the overlay must classify SILENT — the same logical
//! node seen by two producers — not as identity contention.
//!
//! Pre-#561 the HIR producer emitted ABSOLUTE `file` props while syn
//! emits workspace-relative ones, so the 54-A classifier (ratified
//! rule: same `file` string ⇒ silent) saw two different files on one
//! id and warned — a false positive on every `--hir` overlay extract.
//! With both producers on the canonical workspace-relative form the
//! representations match and the overlay is silent, as ratified.
//!
//! The fixture makes convergence deterministic: crate `a` calls
//! `b::helper()` with the FULLY-QUALIFIED path, so syn's as-written
//! callee path equals HIR's resolved qname and both derive
//! `callsite:a::caller:b::helper:0` (lib targets — bare identities).

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
    // Fully-qualified call — syn's textual path == HIR's resolved
    // qname, forcing the two :CallSite ids to converge.
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

    // Producer 1: syn — full workspace facts (:Items + :CallSites with
    // workspace-relative file props per #527).
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

    // Producer 2: HIR — resolved call sites through the adapter.
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

    // The ratified 54-A rule: same workspace-relative `file` string ⇒
    // the same logical node ⇒ SILENT. Any IdentityContention here is
    // the #561 false positive.
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

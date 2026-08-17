//! `EnrichEngine` scaffold tests (RFC-056 slice 056-0).
//!
//! `enrich_deprecation` re-characterizes PR #575's pinned behavior through
//! `EnrichEngine` instead of `PetgraphStore` directly — same assertions,
//! same exact warning text, proving the dispatch move is behavior-identical.

use cfdb_core::enrich::EnrichBackend;
use cfdb_core::schema::Keyspace;
use cfdb_core::store::StoreBackend;
use cfdb_petgraph::PetgraphStore;

use crate::EnrichEngine;

fn store_with_empty_keyspace(ks: &Keyspace) -> PetgraphStore {
    let mut store = PetgraphStore::new();
    store.ingest_nodes(ks, vec![]).expect("register keyspace");
    store
}

#[test]
fn deprecation_pins_fixed_report_shape() {
    let ks = Keyspace::new("test");
    let mut store = store_with_empty_keyspace(&ks);
    let mut engine = EnrichEngine::new(&mut store);

    let report = engine.enrich_deprecation(&ks).expect("pass");

    assert!(report.ran);
    assert_eq!(report.facts_scanned, 0);
    assert_eq!(report.attrs_written, 0);
    assert_eq!(report.edges_written, 0);
    assert_eq!(
        report.warnings,
        vec!["enrich_deprecation: facts populated at extraction time by \
             cfdb-extractor::extract_deprecated_attr (#43-C / RFC \
             addendum §A2.2 row 3); no enrichment work to do"
            .to_string()]
    );
}

#[test]
fn deprecation_unknown_keyspace_returns_err() {
    let mut store = PetgraphStore::new();
    let mut engine = EnrichEngine::new(&mut store);
    let ks = Keyspace::new("never");

    let err = engine
        .enrich_deprecation(&ks)
        .expect_err("unknown keyspace must err");

    assert!(format!("{err:?}").contains("UnknownKeyspace"));
}

#[test]
fn unmoved_verbs_fall_through_to_not_implemented_stub() {
    // Sanity check: every verb except enrich_deprecation is still the
    // trait's default stub as of 056-0 — no pass has moved yet.
    let ks = Keyspace::new("test");
    let mut store = store_with_empty_keyspace(&ks);
    let mut engine = EnrichEngine::new(&mut store);

    let report = engine.enrich_rfc_docs(&ks).expect("pass");
    assert!(!report.ran);
    assert!(report.warnings[0].contains("not implemented"));
}

#[test]
fn enrich_engine_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>(_: &T) {}
    let mut store = PetgraphStore::new();
    let engine = EnrichEngine::new(&mut store);
    assert_send_sync(&engine);
}

#[test]
fn require_workspace_ok_path_matches_the_attached_root() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut store = PetgraphStore::new().with_workspace(tmp.path());
    let engine = EnrichEngine::new(&mut store);

    let root = engine
        .require_workspace("enrich_test_verb", "so the pass can do X")
        .expect("workspace_root is attached");

    assert_eq!(root, tmp.path());
}

#[test]
fn require_workspace_degraded_report_pins_exact_warning_text() {
    // Byte-identical to cfdb-petgraph::enrich_backend.rs's require_workspace
    // (moved verbatim, RFC-056 §3.2) — a pass that migrates in a later
    // slice must see the exact same warning its characterization test
    // (PR #575) already pins.
    let mut store = PetgraphStore::new();
    let engine = EnrichEngine::new(&mut store);

    let report = engine
        .require_workspace("enrich_test_verb", "so the pass can do X")
        .expect_err("no workspace_root attached must degrade, not panic");

    assert_eq!(report.verb, "enrich_test_verb");
    assert!(!report.ran);
    assert_eq!(report.facts_scanned, 0);
    assert_eq!(report.attrs_written, 0);
    assert_eq!(report.edges_written, 0);
    assert_eq!(
        report.warnings,
        vec![
            "enrich_test_verb: no workspace_root attached to PetgraphStore — \
             construct via `PetgraphStore::new().with_workspace(root)` so \
             the pass can do X"
                .to_string()
        ]
    );
}

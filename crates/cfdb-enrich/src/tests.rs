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

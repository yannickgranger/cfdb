//! Characterization tests for the `EnrichBackend::enrich_metrics` dispatch
//! path in `enrich_backend.rs` — NOT for `metrics::run`'s own computation
//! (see `ast_signals.rs` / `clustering.rs` / `coverage.rs` for those).
//!
//! Pre-strangler-fig safety net: these pin what the dispatcher does today
//! (guard #1 `require_keyspace`, guard #2 `require_workspace`) so an
//! enrichment-crate extraction can be verified byte-for-byte against this
//! baseline. Correctness of the pinned shape is a separate, later question.

use cfdb_core::enrich::EnrichBackend;
use cfdb_core::schema::Keyspace;
use cfdb_core::store::StoreBackend;

use crate::PetgraphStore;

#[test]
fn unknown_keyspace_returns_err() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut store = PetgraphStore::new().with_workspace(tmp.path());
    let ks = Keyspace::new("never");
    let err = store
        .enrich_metrics(&ks)
        .expect_err("unknown keyspace must err");
    assert!(format!("{err:?}").contains("UnknownKeyspace"));
}

#[test]
fn no_workspace_root_returns_degraded_report() {
    let mut store = PetgraphStore::new();
    let ks = Keyspace::new("test");
    store.ingest_nodes(&ks, vec![]).expect("register keyspace");

    let report = store.enrich_metrics(&ks).expect("pass");

    assert!(!report.ran, "no workspace_root → ran=false");
    assert_eq!(report.facts_scanned, 0);
    assert_eq!(report.attrs_written, 0);
    assert_eq!(report.edges_written, 0);
    assert_eq!(
        report.warnings,
        vec![
            "enrich_metrics: no workspace_root attached to PetgraphStore — \
             construct via `PetgraphStore::new().with_workspace(root)` so \
             the pass can re-parse source files referenced by \
             :Item{kind:Fn}.file"
                .to_string()
        ]
    );
}

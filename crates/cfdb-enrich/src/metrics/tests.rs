//! Tests for `EnrichEngine::enrich_metrics`'s dispatch guards — NOT for
//! `metrics::run`'s own computation (see `ast_signals.rs` / `clustering.rs`
//! / `coverage.rs` for those).

use cfdb_core::enrich::EnrichBackend;
use cfdb_core::schema::Keyspace;
use cfdb_core::store::StoreBackend;
use cfdb_petgraph::PetgraphStore;

use crate::EnrichEngine;

#[test]
fn unknown_keyspace_returns_err() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut store = PetgraphStore::new().with_workspace(tmp.path());
    let ks = Keyspace::new("never");
    let err = EnrichEngine::new(&mut store)
        .enrich_metrics(&ks)
        .expect_err("unknown keyspace must err");
    assert!(format!("{err:?}").contains("UnknownKeyspace"));
}

#[test]
fn no_workspace_root_returns_degraded_report() {
    let mut store = PetgraphStore::new();
    let ks = Keyspace::new("test");
    store.ingest_nodes(&ks, vec![]).expect("register keyspace");

    let report = EnrichEngine::new(&mut store)
        .enrich_metrics(&ks)
        .expect("pass");

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

#[test]
fn unknown_keyspace_errs_even_when_workspace_root_is_also_missing() {
    // The keyspace guard wins when both fail — never the degraded report.
    let mut store = PetgraphStore::new(); // no workspace root
    let ks = Keyspace::new("never"); // and no such keyspace
    let err = EnrichEngine::new(&mut store)
        .enrich_metrics(&ks)
        .expect_err("keyspace guard must win over the workspace guard");
    assert!(format!("{err:?}").contains("UnknownKeyspace"));
}

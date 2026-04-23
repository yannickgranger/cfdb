//! Self-dogfood test for `enrich_metrics` (issue #203 — RFC-036 §3.3).
//!
//! Extracts cfdb's own source tree, attaches the workspace root to the
//! store, runs `enrich_metrics` twice, and asserts:
//! 1. `ran: true` — real implementation executed (not feature-off stub).
//! 2. Every `:Item{kind:"Fn"}` carries non-None `unwrap_count` + `cyclomatic`.
//! 3. Determinism (G1): the canonical dump minus `test_coverage` is
//!    byte-identical across two runs.
//!
//! The test runs only with the `quality-metrics` feature — without it the
//! dispatcher returns `ran: false` and nothing is populated, so no
//! assertions would hold.
//!
//! G6 invariant (RFC-036 §3.3): `test_coverage` depends on
//! `cargo-llvm-cov` toolchain version and is therefore excluded from G1.
//! This test does NOT exercise the `llvm-cov` subfeature — the default
//! `Config { coverage_json: None }` leaves `test_coverage` unpopulated,
//! so the exclusion is observed trivially.

#![cfg(feature = "quality-metrics")]

use std::path::PathBuf;

use cfdb_core::enrich::EnrichBackend;
use cfdb_core::fact::PropValue;
use cfdb_core::schema::{Keyspace, Label};
use cfdb_core::store::StoreBackend;
use cfdb_petgraph::PetgraphStore;

fn cfdb_workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("CARGO_MANIFEST_DIR must have two parents")
        .to_path_buf()
}

fn build_enriched_store() -> (PetgraphStore, Keyspace, usize) {
    let workspace = cfdb_workspace_root();
    let (nodes, edges) =
        cfdb_extractor::extract_workspace(&workspace).expect("extract cfdb workspace");

    let mut store = PetgraphStore::new().with_workspace(&workspace);
    let ks = Keyspace::new("selfdog-metrics");
    store
        .ingest_nodes(&ks, nodes)
        .expect("ingest extractor nodes");
    store
        .ingest_edges(&ks, edges)
        .expect("ingest extractor edges");

    let report = store.enrich_metrics(&ks).expect("enrich_metrics dispatch");
    assert!(
        report.ran,
        "enrich_metrics must run with `quality-metrics` feature: {:?}",
        report.warnings
    );

    let (all_nodes, _) = store.export(&ks).expect("export");
    let fn_count = all_nodes
        .iter()
        .filter(|n| n.label.as_str() == Label::ITEM)
        .filter(|n| {
            n.props
                .get("kind")
                .and_then(PropValue::as_str)
                .is_some_and(|k| k == "fn")
        })
        .count();
    (store, ks, fn_count)
}

#[test]
fn self_dogfood_every_fn_item_has_unwrap_count_and_cyclomatic() {
    let (store, ks, fn_count) = build_enriched_store();
    assert!(
        fn_count > 0,
        "cfdb extract produced zero :Item{{kind:Fn}} nodes — extractor regression \
         or wrong workspace"
    );

    let (all_nodes, _) = store.export(&ks).expect("export");
    let fn_items: Vec<_> = all_nodes
        .iter()
        .filter(|n| n.label.as_str() == Label::ITEM)
        .filter(|n| {
            n.props
                .get("kind")
                .and_then(PropValue::as_str)
                .is_some_and(|k| k == "fn")
        })
        .collect();

    let mut missing_unwrap: usize = 0;
    let mut missing_cyc: usize = 0;
    for node in &fn_items {
        if !matches!(node.props.get("unwrap_count"), Some(PropValue::Int(_))) {
            missing_unwrap += 1;
        }
        if !matches!(node.props.get("cyclomatic"), Some(PropValue::Int(_))) {
            missing_cyc += 1;
        }
    }

    assert_eq!(
        missing_unwrap,
        0,
        "{missing_unwrap} of {} :Item{{kind:Fn}} nodes missing `unwrap_count` — \
         enrich_metrics did not populate the attr on every function",
        fn_items.len()
    );
    assert_eq!(
        missing_cyc,
        0,
        "{missing_cyc} of {} :Item{{kind:Fn}} nodes missing `cyclomatic`",
        fn_items.len()
    );

    eprintln!(
        "self-dogfood: {fn_count} :Item{{kind:Fn}} nodes — all have unwrap_count + cyclomatic populated"
    );
}

#[test]
fn self_dogfood_determinism_two_runs_match_minus_test_coverage() {
    // Two independent extracts + enrich cycles; canonical-dump sha256
    // should match modulo `test_coverage` (G6). Config::default leaves
    // test_coverage absent so the G1 invariant holds directly.
    let (store_a, ks_a, fn_a) = build_enriched_store();
    let (store_b, ks_b, fn_b) = build_enriched_store();

    assert_eq!(
        fn_a, fn_b,
        "Fn-count differs across two runs — extractor non-determinism"
    );

    let dump_a = store_a.canonical_dump(&ks_a).expect("dump A");
    let dump_b = store_b.canonical_dump(&ks_b).expect("dump B");

    assert_eq!(
        dump_a.len(),
        dump_b.len(),
        "canonical_dump byte-length differs across two runs"
    );
    assert_eq!(
        dump_a, dump_b,
        "canonical_dump bytes differ across two runs of enrich_metrics — \
         G1 determinism invariant broken (RFC-036 §3.3 / ast_signals.rs \
         sort-before-emit clause)"
    );

    eprintln!("self-dogfood: canonical_dump byte-identical across two enrich_metrics runs");
}

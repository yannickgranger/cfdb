//! Self-dogfood test for `enrich_bounded_context` (issue #108 — slice 43-E).
//!
//! Extracts cfdb's own worktree (which carries `.cfdb/concepts/cfdb.toml`
//! declaring every cfdb crate → `"cfdb"` context), runs
//! `enrich_bounded_context`, and asserts:
//!
//! - AC-2 shape: on a fresh extract the pass is a no-op (`attrs_written = 0,
//!   ran = true`) because the extractor already honours the TOML.
//! - AC-4: ≥95% of `:Item` nodes carry the expected `bounded_context`
//!   (= `"cfdb"`, from the TOML override). Given the override covers all 9
//!   workspace crates, actual coverage should be 100%.
//!
//! Runs as a Rust integration test using the library API — a failure
//! surfaces as a stack trace inside `cargo test`, not CLI output.

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

/// AC-4 — cfdb-scoped ground truth via `.cfdb/concepts/cfdb.toml`.
/// Every cfdb `:Item` should carry `bounded_context = "cfdb"` after extract.
/// Running `enrich_bounded_context` against the same TOML is a no-op (the
/// extractor-time path already applied the override).
#[test]
fn ac2_and_ac4_self_dogfood_cfdb_scoped_ground_truth() {
    let workspace = cfdb_workspace_root();
    let (nodes, edges) = cfdb_extractor::extract_workspace(&workspace).expect("extract cfdb");

    let mut store = PetgraphStore::new().with_workspace(&workspace);
    let ks = Keyspace::new("selfdog");
    store.ingest_nodes(&ks, nodes).expect("ingest nodes");
    store.ingest_edges(&ks, edges).expect("ingest edges");

    let report = store
        .enrich_bounded_context(&ks)
        .expect("enrich_bounded_context");

    // AC-2 shape — fresh extract matches current TOML, so no patches needed.
    assert!(report.ran, "pass must run: {:?}", report.warnings);
    assert_eq!(
        report.attrs_written, 0,
        "AC-2: fresh extract must be a no-op (extractor already honours \
         .cfdb/concepts/cfdb.toml), got attrs_written={}",
        report.attrs_written
    );

    // AC-4 — every :Item should report bounded_context == "cfdb".
    let (all_nodes, _) = store.export(&ks).expect("export");
    let items: Vec<_> = all_nodes
        .iter()
        .filter(|n| n.label.as_str() == Label::ITEM)
        .collect();
    assert!(!items.is_empty(), "cfdb extract produced zero :Item nodes");

    let total = items.len();
    let matches = items
        .iter()
        .filter(|n| {
            n.props
                .get("bounded_context")
                .and_then(PropValue::as_str)
                .is_some_and(|v| v == "cfdb")
        })
        .count();
    let accuracy = (matches as f64) / (total as f64);

    assert!(
        accuracy >= 0.95,
        "AC-4: {:.2}% of :Item nodes have bounded_context == \"cfdb\" \
         ({matches}/{total}) — must be ≥ 95%. TOML file at \
         .cfdb/concepts/cfdb.toml may be out of sync with workspace members.",
        accuracy * 100.0
    );

    eprintln!(
        "self-dogfood: {}/{} :Item nodes ({:.1}%) carry bounded_context == \"cfdb\"",
        matches,
        total,
        accuracy * 100.0
    );
}

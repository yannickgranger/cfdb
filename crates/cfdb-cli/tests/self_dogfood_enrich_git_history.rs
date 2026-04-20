//! Self-dogfood test for `enrich_git_history` (issue #105 — slice 43-B).
//!
//! Extracts cfdb's own source tree, attaches the workspace root to the store,
//! runs `enrich_git_history`, and asserts that ≥80% of `:Item` nodes pick up
//! a non-null `git_last_commit_unix_ts`. This is AC-4 + AC-5 from the issue
//! body — exercised as a Rust integration test rather than a shell script so
//! the failure mode is a stack trace inside `cargo test`, not a wall of CLI
//! output.
//!
//! The test runs only with the `git-enrich` feature (no git2, nothing to
//! populate → nothing to assert on) and resolves the cfdb workspace root
//! from `CARGO_MANIFEST_DIR` — this keeps the test portable across
//! worktrees, CI runners, and user clones.

#![cfg(feature = "git-enrich")]

use std::path::PathBuf;

use cfdb_core::enrich::EnrichBackend;
use cfdb_core::fact::PropValue;
use cfdb_core::schema::{Keyspace, Label};
use cfdb_core::store::StoreBackend;
use cfdb_petgraph::PetgraphStore;

fn cfdb_workspace_root() -> PathBuf {
    // CARGO_MANIFEST_DIR for this test is `<workspace>/crates/cfdb-cli`. Pop
    // two levels to reach the workspace root. Using `env!` (not `option_env!`)
    // because `CARGO_MANIFEST_DIR` is always set under `cargo test`; a missing
    // value indicates a broken test runner and should be a compile error.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("CARGO_MANIFEST_DIR must have two parents")
        .to_path_buf()
}

#[test]
fn ac4_ac5_self_dogfood_eighty_percent_items_have_git_attrs() {
    let workspace = cfdb_workspace_root();

    // Extract → ingest into a PetgraphStore with workspace_root attached.
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

    // Run the enrichment pass.
    let report = store.enrich_git_history(&ks).expect("enrich_git_history");
    assert!(
        report.ran,
        "enrich_git_history must actually run on a git-tracked workspace: {:?}",
        report.warnings
    );

    // AC-4: attrs_written >= count_of_items_in_tracked_files. Every :Item got
    // three attrs (Null or real), so attrs_written >= 3 × item_count / 3 = item_count.
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

    // AC-5: ≥80% of :Item nodes have non-null git_last_commit_unix_ts.
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

//! `cfdb enrich-reachability` end-to-end through the real binary, against
//! cfdb's own tree.
//!
//! The reachability BFS only runs when the keyspace carries at least one
//! `:EntryPoint`; a syn-only extract of cfdb-self carries none (entry
//! points come from the HIR extractor). So the test extracts cfdb-self,
//! then injects one synthetic `:EntryPoint -[:EXPOSES]-> :Item` seed
//! straight into the on-disk keyspace file, and asserts through the
//! binary that:
//!
//! 1. the report says the pass ran and wrote attributes;
//! 2. the enriched keyspace was persisted — the seed `:Item` carries all
//!    four reach/count attributes (both the All and the ProductionOnly
//!    pass wrote), and every `:Item` in the keyspace carries the
//!    `reachable_from_entry` attribute (never left null).

use std::path::{Path, PathBuf};

use assert_cmd::prelude::*;
use serde_json::{json, Value};
use tempfile::tempdir;

const SEED_ITEM_ID: &str = "item:__synthetic::__test_seed";
const SEED_ENTRY_ID: &str = "entrypoint:cli_command:__synthetic::__test_seed";

fn cfdb_workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("CARGO_MANIFEST_DIR must have two parents")
        .to_path_buf()
}

fn extract_selfdog(workspace: &Path, db_path: &Path) {
    std::process::Command::cargo_bin("cfdb")
        .expect("cfdb binary is built for integration tests")
        .args([
            "extract",
            "--workspace",
            workspace.to_str().expect("utf-8 path"),
            "--db",
            db_path.to_str().expect("utf-8 path"),
            "--keyspace",
            "selfdog",
        ])
        .assert()
        .success();
}

/// Append one `:EntryPoint -[:EXPOSES]-> :Item` seed to the persisted
/// keyspace file so the BFS has something to walk from.
fn inject_synthetic_seed(keyspace_file: &Path) {
    let raw = std::fs::read(keyspace_file).expect("read keyspace file");
    let mut file: Value = serde_json::from_slice(&raw).expect("keyspace file is JSON");
    file["nodes"].as_array_mut().expect("nodes array").extend([
        json!({
            "id": SEED_ENTRY_ID,
            "label": "EntryPoint",
            "props": { "kind": "cli_command" }
        }),
        json!({
            "id": SEED_ITEM_ID,
            "label": "Item",
            "props": {
                "qname": "__synthetic::__test_seed",
                "name": "__test_seed",
                "kind": "fn",
                "crate": "__synthetic",
                "file": "__test__.rs",
                "is_test": false
            }
        }),
    ]);
    file["edges"]
        .as_array_mut()
        .expect("edges array")
        .push(json!({
            "src": SEED_ENTRY_ID,
            "dst": SEED_ITEM_ID,
            "label": "EXPOSES"
        }));
    std::fs::write(keyspace_file, serde_json::to_vec(&file).expect("serialize"))
        .expect("write keyspace file");
}

#[test]
fn enrich_reachability_through_the_real_binary_on_cfdb_self() {
    let workspace = cfdb_workspace_root();
    let db = tempdir().expect("tempdir");
    let db_path: PathBuf = db.path().to_path_buf();
    extract_selfdog(&workspace, &db_path);

    let keyspace_file = db_path.join("selfdog.json");
    inject_synthetic_seed(&keyspace_file);

    let output = std::process::Command::cargo_bin("cfdb")
        .expect("cfdb binary is built for integration tests")
        .args([
            "enrich-reachability",
            "--db",
            db_path.to_str().expect("utf-8 path"),
            "--keyspace",
            "selfdog",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let report: Value =
        serde_json::from_slice(&output).expect("enrich-reachability prints EnrichReport JSON");
    assert_eq!(report["verb"], "enrich_reachability");
    assert_eq!(report["ran"], true, "report: {report}");
    assert_eq!(
        report["facts_scanned"], 1,
        "exactly the one injected :EntryPoint is scanned: {report}"
    );
    assert!(
        report["attrs_written"].as_u64().expect("u64") > 0,
        "every :Item gets reach/count attrs from both passes: {report}"
    );

    // The pass mutated the graph, so the CLI must have persisted it.
    let raw = std::fs::read(&keyspace_file).expect("re-read keyspace file");
    let file: Value = serde_json::from_slice(&raw).expect("keyspace file is JSON");
    let items: Vec<&Value> = file["nodes"]
        .as_array()
        .expect("nodes array")
        .iter()
        .filter(|n| n["label"] == "Item")
        .collect();
    assert!(
        !items.is_empty(),
        "cfdb-self extract must contain :Item nodes"
    );

    let seed = items
        .iter()
        .find(|n| n["id"] == SEED_ITEM_ID)
        .expect("injected seed :Item survives the round-trip");
    assert_eq!(seed["props"]["reachable_from_entry"], true, "seed: {seed}");
    assert_eq!(seed["props"]["reachable_entry_count"], 1, "seed: {seed}");
    assert_eq!(
        seed["props"]["reachable_from_production_entry"], true,
        "cli_command is a production entry kind: {seed}"
    );
    assert_eq!(
        seed["props"]["reachable_production_entry_count"], 1,
        "seed: {seed}"
    );

    let unmarked = items
        .iter()
        .filter(|n| !n["props"]["reachable_from_entry"].is_boolean())
        .count();
    assert_eq!(
        unmarked, 0,
        "every :Item must carry a boolean reachable_from_entry — never left null"
    );
}

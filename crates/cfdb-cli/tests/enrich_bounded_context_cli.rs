use std::path::PathBuf;

use assert_cmd::prelude::*;
use tempfile::tempdir;

fn cfdb_workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("CARGO_MANIFEST_DIR must have two parents")
        .to_path_buf()
}

#[test]
fn enrich_bounded_context_through_the_real_binary_on_cfdb_self() {
    let workspace = cfdb_workspace_root();
    let db = tempdir().expect("tempdir");
    let db_path: PathBuf = db.path().to_path_buf();

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

    let output = std::process::Command::cargo_bin("cfdb")
        .expect("cfdb binary is built for integration tests")
        .args([
            "enrich-bounded-context",
            "--db",
            db_path.to_str().expect("utf-8 path"),
            "--keyspace",
            "selfdog",
            "--workspace",
            workspace.to_str().expect("utf-8 path"),
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let report: serde_json::Value =
        serde_json::from_slice(&output).expect("enrich-bounded-context prints EnrichReport JSON");

    assert_eq!(report["verb"], "enrich_bounded_context");
    assert_eq!(report["ran"], true, "report: {report}");
    assert!(
        report["facts_scanned"].as_u64().expect("u64") > 0,
        "cfdb's own :Item nodes must be scanned: {report}"
    );
    assert_eq!(
        report["attrs_written"], 0,
        "fresh extract must already honour .cfdb/concepts/cfdb.toml: {report}"
    );
}

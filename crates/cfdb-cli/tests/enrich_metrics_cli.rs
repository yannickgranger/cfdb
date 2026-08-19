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

fn extract_selfdog(workspace: &std::path::Path, db_path: &std::path::Path) {
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

#[cfg(feature = "quality-metrics")]
#[test]
fn enrich_metrics_through_the_real_binary_on_cfdb_self() {
    let workspace = cfdb_workspace_root();
    let db = tempdir().expect("tempdir");
    let db_path: PathBuf = db.path().to_path_buf();
    extract_selfdog(&workspace, &db_path);

    let output = std::process::Command::cargo_bin("cfdb")
        .expect("cfdb binary is built for integration tests")
        .args([
            "enrich-metrics",
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
        serde_json::from_slice(&output).expect("enrich-metrics prints EnrichReport JSON");

    assert_eq!(report["verb"], "enrich_metrics");
    assert_eq!(report["ran"], true, "report: {report}");
    assert!(
        report["attrs_written"].as_u64().expect("u64") > 0,
        "cfdb's own :Item{{kind:Fn}} nodes must get unwrap_count + cyclomatic: {report}"
    );
}

#[cfg(not(feature = "quality-metrics"))]
#[test]
fn enrich_metrics_feature_off_degraded_report_through_the_real_binary() {
    let workspace = cfdb_workspace_root();
    let db = tempdir().expect("tempdir");
    let db_path: PathBuf = db.path().to_path_buf();
    extract_selfdog(&workspace, &db_path);

    let output = std::process::Command::cargo_bin("cfdb")
        .expect("cfdb binary is built for integration tests")
        .args([
            "enrich-metrics",
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
        serde_json::from_slice(&output).expect("enrich-metrics prints EnrichReport JSON");

    assert_eq!(report["verb"], "enrich_metrics");
    assert_eq!(report["ran"], false, "report: {report}");
    let warnings = report["warnings"].as_array().expect("warnings array");
    assert_eq!(warnings.len(), 1);
    assert!(
        warnings[0]
            .as_str()
            .expect("string")
            .contains("built without `quality-metrics` feature"),
        "dispatcher must route to EnrichEngine's specific feature-off warning, \
         not a generic not-implemented stub: {report}"
    );
}

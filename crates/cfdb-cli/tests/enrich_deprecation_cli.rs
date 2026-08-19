use std::path::PathBuf;
use std::process::Command;

use assert_cmd::prelude::*;
use tempfile::tempdir;

fn minimal_fixture_workspace() -> tempfile::TempDir {
    let dir = tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .expect("write Cargo.toml");
    std::fs::create_dir(dir.path().join("src")).expect("mkdir src");
    std::fs::write(dir.path().join("src/lib.rs"), "pub fn f() {}\n").expect("write lib.rs");
    dir
}

#[test]
fn enrich_deprecation_through_the_real_binary_matches_pinned_report() {
    let workspace = minimal_fixture_workspace();
    let db = tempdir().expect("tempdir");
    let db_path: PathBuf = db.path().to_path_buf();

    Command::cargo_bin("cfdb")
        .expect("cfdb binary is built for integration tests")
        .args([
            "extract",
            "--workspace",
            workspace.path().to_str().expect("utf-8 path"),
            "--db",
            db_path.to_str().expect("utf-8 path"),
            "--keyspace",
            "selfdog",
        ])
        .assert()
        .success();

    let output = Command::cargo_bin("cfdb")
        .expect("cfdb binary is built for integration tests")
        .args([
            "enrich-deprecation",
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

    let report: serde_json::Value =
        serde_json::from_slice(&output).expect("enrich-deprecation prints EnrichReport JSON");

    assert_eq!(report["verb"], "enrich_deprecation");
    assert_eq!(report["ran"], true);
    assert_eq!(report["facts_scanned"], 0);
    assert_eq!(report["attrs_written"], 0);
    assert_eq!(report["edges_written"], 0);
    assert_eq!(
        report["warnings"][0],
        "enrich_deprecation: facts populated at extraction time by \
         cfdb-extractor::extract_deprecated_attr (#43-C / RFC addendum \
         §A2.2 row 3); no enrichment work to do"
    );
}

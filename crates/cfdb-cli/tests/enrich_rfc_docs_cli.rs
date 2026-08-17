//! `cfdb enrich-rfc-docs` end-to-end through the real binary, against
//! cfdb's own tree (RFC-056 slice 056-A / issue #578).
//!
//! Unlike `enrich_deprecation_cli.rs` (056-0's CLI test, which is safe to
//! run against a minimal synthetic fixture because `enrich_deprecation`'s
//! report is content-independent), `enrich_rfc_docs`'s report depends on
//! real `docs/*.md` content actually referencing real `:Item`s — a
//! synthetic single-file fixture would prove nothing about the
//! composition-root cutover. This is the template for 056-B..F's CLI
//! tests: self-dogfood, shelled out through the real binary, not a
//! synthetic fixture.
//!
//! `self_dogfood_enrich_rfc_docs.rs` already exercises `EnrichEngine`
//! in-process on cfdb-self; this test's only additional job is proving
//! `crates/cfdb-cli/src/enrich.rs`'s dispatcher actually routes
//! `EnrichVerb::RfcDocs` to it — a botched cutover (e.g. dispatcher still
//! calling the now-stubbed `PetgraphStore::enrich_rfc_docs`) shows up here
//! as `ran: false` even though the in-process test would still pass.

use std::path::PathBuf;
use std::process::Command;

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
fn enrich_rfc_docs_through_the_real_binary_on_cfdb_self() {
    let workspace = cfdb_workspace_root();
    let db = tempdir().expect("tempdir");
    let db_path: PathBuf = db.path().to_path_buf();

    Command::cargo_bin("cfdb")
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

    let output = Command::cargo_bin("cfdb")
        .expect("cfdb binary is built for integration tests")
        .args([
            "enrich-rfc-docs",
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
        serde_json::from_slice(&output).expect("enrich-rfc-docs prints EnrichReport JSON");

    assert_eq!(report["verb"], "enrich_rfc_docs");
    assert_eq!(report["ran"], true, "report: {report}");
    assert!(
        report["facts_scanned"].as_u64().expect("u64") > 0,
        "cfdb's own docs/*.md must be scanned: {report}"
    );
    assert!(
        report["edges_written"].as_u64().expect("u64") > 0,
        "cfdb's own RFCs reference real :Item nodes: {report}"
    );
}

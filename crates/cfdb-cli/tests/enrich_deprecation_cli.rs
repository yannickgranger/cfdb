//! `cfdb enrich-deprecation` end-to-end through the real binary (RFC-056
//! 056-0 — the one verb whose composition-root cutover to `EnrichEngine`
//! landed in 056-0 itself, per `crates/cfdb-cli/src/enrich.rs`).
//!
//! NOT a `self_dogfood_*` test (deliberately not named that way): it runs
//! against a minimal synthetic single-crate fixture, not cfdb's own tree —
//! harmless here because `enrich_deprecation`'s report is content-independent
//! (fixed counters, fixed warning text), but the wrong template for the
//! other 6 verbs once they move (056-A..F), which DO need a cfdb-self
//! fixture to prove diff-emptiness per RFC-056 §3.4. This file's only job
//! is proving the CLI→EnrichEngine wiring — unlike the in-process
//! `self_dogfood_enrich_*.rs` tests (which construct `PetgraphStore`
//! directly and never exercise `crates/cfdb-cli/src/enrich.rs`'s
//! dispatcher), this shells out to the actual `cfdb` binary — the same
//! path `tools/dogfood-enrich` drives — so a botched composition-root
//! cutover shows up here even though it wouldn't in the in-process tests.

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

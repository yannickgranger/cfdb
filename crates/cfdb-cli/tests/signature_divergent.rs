use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use assert_cmd::prelude::*;
use tempfile::tempdir;

fn queries_dir() -> PathBuf {
    let cfdb_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("cfdb-cli crate dir has a parent crates/")
        .parent()
        .expect("crates/ has a parent cfdb sub-workspace root");
    cfdb_root.join("examples/queries")
}

fn fixture_dir() -> PathBuf {
    queries_dir().join("fixtures/signature-divergent")
}

fn rule(name: &str) -> PathBuf {
    queries_dir().join(name)
}

fn copy_fixture(dst: &Path) {
    copy_dir_recursive(&fixture_dir(), dst);
}

fn copy_dir_recursive(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).expect("mkdir fixture dst");
    for entry in fs::read_dir(src).expect("read fixture src") {
        let entry = entry.expect("fixture dir entry");
        let ft = entry.file_type().expect("fixture file type");
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if ft.is_dir() {
            copy_dir_recursive(&from, &to);
        } else if ft.is_file() {
            fs::copy(&from, &to).expect("copy fixture file");
        }
    }
}

fn cfdb() -> Command {
    Command::cargo_bin("cfdb").expect("cfdb binary is built for integration tests")
}

fn extract(tmp: &Path) -> (PathBuf, &'static str) {
    let workspace = tmp.join("workspace");
    copy_fixture(&workspace);
    let db = tmp.join("db");
    let ks = "sigdiv";

    cfdb()
        .args([
            "extract",
            "--workspace",
            workspace.to_str().expect("workspace path utf-8"),
            "--db",
            db.to_str().expect("db path utf-8"),
            "--keyspace",
            ks,
        ])
        .assert()
        .success();

    (db, ks)
}

fn run_rule(db: &Path, ks: &str, rule_file: &Path) -> String {
    let cypher = fs::read_to_string(rule_file).expect("read rule file");
    let output = cfdb()
        .args([
            "query",
            "--db",
            db.to_str().expect("db path utf-8"),
            "--keyspace",
            ks,
            &cypher,
        ])
        .output()
        .expect("run cfdb query");
    String::from_utf8(output.stdout).expect("query stdout utf-8")
}

#[test]
fn divergent_valuation_pair_surfaces_as_context_homonym() {
    let tmp = tempdir().expect("tempdir");
    let (db, ks) = extract(tmp.path());
    let stdout = run_rule(&db, ks, &rule("signature-divergent.cypher"));

    assert!(
        stdout.contains("valuation"),
        "Position::valuation has DIVERGENT signatures across trading_port \
         and trading_adapter (fn(&Self) -> f64 vs fn(&Self) -> (f64, f64)) \
         — must surface under signature-divergent.cypher:\n{stdout}"
    );
    assert!(
        stdout.contains("trading_port"),
        "DIVERGENT pair row must cite trading_port as one of the bounded \
         contexts:\n{stdout}"
    );
    assert!(
        stdout.contains("trading_adapter"),
        "DIVERGENT pair row must cite trading_adapter as one of the \
         bounded contexts:\n{stdout}"
    );
}

#[test]
fn identical_place_order_pair_is_not_surfaced_as_homonym() {
    let tmp = tempdir().expect("tempdir");
    let (db, ks) = extract(tmp.path());
    let stdout = run_rule(&db, ks, &rule("signature-divergent.cypher"));

    assert!(
        !stdout.contains("place_order"),
        "OrderBook::place_order has IDENTICAL signatures across \
         trading_port and trading_adapter (Shared Kernel) — must NOT \
         surface under signature-divergent.cypher (route to \
         /sweep-epic would incorrectly dedupe intentional shared \
         contract):\n{stdout}"
    );
}

#[test]
fn signature_emission_is_byte_stable_across_extracts() {
    let tmp = tempdir().expect("tempdir");
    let (db, ks) = extract(tmp.path());
    let out1 = run_rule(&db, ks, &rule("signature-divergent.cypher"));

    let tmp2 = tempdir().expect("tempdir2");
    let (db2, ks2) = extract(tmp2.path());
    let out2 = run_rule(&db2, ks2, &rule("signature-divergent.cypher"));

    assert_eq!(
        out1, out2,
        "signature-divergent.cypher output diverged across two extracts \
         of the same fixture — `:Item.signature` emission is not \
         byte-stable (G1 violation)."
    );
}

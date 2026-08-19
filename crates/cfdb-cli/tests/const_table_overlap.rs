use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use assert_cmd::prelude::*;
use tempfile::tempdir;

fn queries_dir() -> PathBuf {
    let cfdb_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("cfdb-cli has parent crates/")
        .parent()
        .expect("crates/ has parent cfdb sub-workspace root");
    cfdb_root.join("examples/queries")
}

fn fixture_dir() -> PathBuf {
    queries_dir().join("fixtures/const-table-overlap")
}

fn rule(name: &str) -> PathBuf {
    queries_dir().join(name)
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
    Command::cargo_bin("cfdb").expect("cfdb binary built for integration tests")
}

fn extract(tmp: &Path) -> (PathBuf, &'static str) {
    let workspace = tmp.join("workspace");
    copy_dir_recursive(&fixture_dir(), &workspace);
    let db = tmp.join("db");
    let ks = "ctoverlap";

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
fn duplicate_fiat_pair_surfaces_under_const_table_overlap() {
    let tmp = tempdir().expect("tempdir");
    let (db, ks) = extract(tmp.path());
    let stdout = run_rule(&db, ks, &rule("const-table-overlap.cypher"));

    assert!(
        stdout.contains("CONST_TABLE_DUPLICATE"),
        "DUPLICATE pair must surface with the verdict label:\n{stdout}"
    );
    assert!(
        stdout.contains("kraken_normalize::FIAT"),
        "row must cite kraken_normalize::FIAT as one member of the pair:\n{stdout}"
    );
    assert!(
        stdout.contains("oanda_pricing::FIAT"),
        "row must cite oanda_pricing::FIAT as the other member:\n{stdout}"
    );
}

#[test]
fn duplicate_pair_is_reported_once_via_qname_lex_dedup() {
    let tmp = tempdir().expect("tempdir");
    let (db, ks) = extract(tmp.path());
    let stdout = run_rule(&db, ks, &rule("const-table-overlap.cypher"));

    let n = stdout.matches("CONST_TABLE_DUPLICATE").count();
    assert_eq!(
        n, 1,
        "expected exactly one DUPLICATE row (qname lex-dedup); got {n}\n{stdout}"
    );
}

#[test]
fn binance_stables_does_not_pair_with_unrelated_str_tables() {
    let tmp = tempdir().expect("tempdir");
    let (db, ks) = extract(tmp.path());
    let stdout = run_rule(&db, ks, &rule("const-table-overlap.cypher"));

    assert!(
        !stdout.contains("binance_exchange::STABLES"),
        "STABLES has same element_type as FIAT but non-overlapping set — \
         must NOT surface (entries_hash join, not element_type alone):\n{stdout}"
    );
}

#[test]
fn numeric_table_does_not_pair_with_string_tables() {
    let tmp = tempdir().expect("tempdir");
    let (db, ks) = extract(tmp.path());
    let stdout = run_rule(&db, ks, &rule("const-table-overlap.cypher"));

    assert!(
        !stdout.contains("metric_client::PORTS"),
        "PORTS is element_type=u32 and disjoint from every other fixture \
         set; must NOT surface in any row:\n{stdout}"
    );
}

#[test]
fn subset_pair_surfaces_under_const_table_subset() {
    let tmp = tempdir().expect("tempdir");
    let (db, ks) = extract(tmp.path());
    let stdout = run_rule(&db, ks, &rule("const-table-overlap.cypher"));

    assert!(
        stdout.contains("CONST_TABLE_SUBSET"),
        "SUBSET pair must surface with the verdict label:\n{stdout}"
    );
    assert!(
        stdout.contains("kraken_session_ports::PORTS"),
        "SUBSET row must cite kraken_session_ports::PORTS as one \
         member of the pair:\n{stdout}"
    );
    assert!(
        stdout.contains("oanda_session_ports::PORTS"),
        "SUBSET row must cite oanda_session_ports::PORTS as the other \
         member of the pair:\n{stdout}"
    );
}

#[test]
fn intersection_high_pair_surfaces_under_const_table_intersection_high() {
    let tmp = tempdir().expect("tempdir");
    let (db, ks) = extract(tmp.path());
    let stdout = run_rule(&db, ks, &rule("const-table-overlap.cypher"));

    assert!(
        stdout.contains("CONST_TABLE_INTERSECTION_HIGH"),
        "INTERSECTION_HIGH pair must surface with the verdict label:\n{stdout}"
    );
    assert!(
        stdout.contains("mt5_jaccard_ports::PORTS"),
        "INTERSECTION_HIGH row must cite mt5_jaccard_ports::PORTS:\n{stdout}"
    );
}

#[test]
fn below_threshold_pair_does_not_surface() {
    let tmp = tempdir().expect("tempdir");
    let (db, ks) = extract(tmp.path());
    let stdout = run_rule(&db, ks, &rule("const-table-overlap.cypher"));

    let dup = stdout.matches("CONST_TABLE_DUPLICATE").count();
    let sub = stdout.matches("CONST_TABLE_SUBSET").count();
    let high = stdout.matches("CONST_TABLE_INTERSECTION_HIGH").count();
    let none = stdout.matches("CONST_TABLE_NONE").count();
    assert_eq!(
        dup, 1,
        "expected exactly one DUPLICATE row; got {dup}:\n{stdout}"
    );
    assert_eq!(
        sub, 1,
        "expected exactly one SUBSET row; got {sub}:\n{stdout}"
    );
    assert_eq!(
        high, 1,
        "expected exactly one INTERSECTION_HIGH row; got {high}:\n{stdout}"
    );
    assert_eq!(
        none, 0,
        "rule must filter CONST_TABLE_NONE rows via the trailing \
         WITH/WHERE; got {none}:\n{stdout}"
    );
}

#[test]
fn const_table_overlap_rule_output_is_byte_stable() {
    let tmp1 = tempdir().expect("tempdir1");
    let tmp2 = tempdir().expect("tempdir2");
    let (db1, ks1) = extract(tmp1.path());
    let (db2, ks2) = extract(tmp2.path());
    let out1 = run_rule(&db1, ks1, &rule("const-table-overlap.cypher"));
    let out2 = run_rule(&db2, ks2, &rule("const-table-overlap.cypher"));
    assert_eq!(
        out1, out2,
        "const-table-overlap.cypher rule output must be byte-identical \
         across two extracts (G1 determinism)"
    );
}

#![cfg(feature = "hir")]

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
    queries_dir().join("fixtures/canonical-bypass")
}

fn rule(name: &str) -> PathBuf {
    queries_dir().join(name)
}

fn copy_fixture(dst: &Path) {
    let src = fixture_dir();
    copy_dir_recursive(&src, dst);
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

const PARAMS_BYPASS: &str =
    r#"{"concept":"ledger","bypass_callee_name":"append","caller_regex":".*::LedgerService::.*"}"#;
const PARAMS_CANONICAL: &str = r#"{"concept":"ledger","canonical_callee_name":"append_idempotent","caller_regex":".*::LedgerService::.*"}"#;
const PARAMS_UNREACHABLE: &str = r#"{"concept":"ledger"}"#;

fn build_and_enrich(tmp: &Path) -> (PathBuf, &'static str) {
    let workspace = tmp.join("workspace");
    copy_fixture(&workspace);
    let db = tmp.join("db");
    let ks = "fixture";

    cfdb()
        .args([
            "extract",
            "--workspace",
            workspace.to_str().expect("workspace path utf-8"),
            "--db",
            db.to_str().expect("db path utf-8"),
            "--keyspace",
            ks,
            "--hir",
            "--no-proc-macro",
        ])
        .assert()
        .success();

    cfdb()
        .args([
            "enrich-concepts",
            "--db",
            db.to_str().expect("db path utf-8"),
            "--keyspace",
            ks,
            "--workspace",
            workspace.to_str().expect("workspace path utf-8"),
        ])
        .assert()
        .success();

    cfdb()
        .args([
            "enrich-reachability",
            "--db",
            db.to_str().expect("db path utf-8"),
            "--keyspace",
            ks,
        ])
        .assert()
        .success();

    (db, ks)
}

fn run_rule(db: &Path, ks: &str, rule_file: &Path, params: &str) -> String {
    let cypher = fs::read_to_string(rule_file).expect("read rule file");
    let output = cfdb()
        .args([
            "query",
            "--db",
            db.to_str().expect("db path utf-8"),
            "--keyspace",
            ks,
            "--params",
            params,
            &cypher,
        ])
        .output()
        .expect("run cfdb query");
    String::from_utf8(output.stdout).expect("query stdout utf-8")
}

#[test]
fn canonical_caller_rule_finds_only_canonical_form_invocations() {
    let tmp = tempdir().expect("tempdir");
    let (db, ks) = build_and_enrich(tmp.path());
    let stdout = run_rule(
        &db,
        ks,
        &rule("canonical-bypass-caller.cypher"),
        PARAMS_CANONICAL,
    );

    assert!(
        stdout.contains("record_trade_safe"),
        "record_trade_safe uses append_idempotent → must surface as CANONICAL_CALLER:\n{stdout}"
    );
    assert!(
        stdout.contains("record_isolated"),
        "record_isolated uses append_idempotent → must surface as CANONICAL_CALLER:\n{stdout}"
    );
    assert!(
        !stdout.contains("record_trade\""),
        "record_trade uses append (bypass) → must NOT surface as CANONICAL_CALLER:\n{stdout}"
    );
    assert!(
        !stdout.contains("record_orphan"),
        "record_orphan uses append (bypass) → must NOT surface as CANONICAL_CALLER:\n{stdout}"
    );
}

#[test]
fn bypass_reachable_rule_reproduces_3525() {
    let tmp = tempdir().expect("tempdir");
    let (db, ks) = build_and_enrich(tmp.path());
    let stdout = run_rule(
        &db,
        ks,
        &rule("canonical-bypass-reachable.cypher"),
        PARAMS_BYPASS,
    );

    assert!(
        stdout.contains("record_trade"),
        "record_trade is reached via cli::run_record and calls the bypass append() \
         — must surface as BYPASS_REACHABLE (reproducing qbot-core #3525):\n{stdout}"
    );
    assert!(
        !stdout.contains("record_orphan"),
        "record_orphan is NOT reached from any :EntryPoint → must NOT surface \
         as BYPASS_REACHABLE (it is BYPASS_DEAD):\n{stdout}"
    );
    assert!(
        !stdout.contains("record_trade_safe"),
        "record_trade_safe uses the canonical form → must NOT surface as BYPASS_*:\n{stdout}"
    );
    assert!(
        !stdout.contains("seed_fixture"),
        "seed_fixture is test-only → is_test filter MUST drop it:\n{stdout}"
    );
}

#[test]
fn bypass_dead_rule_reproduces_3544_3545_3546() {
    let tmp = tempdir().expect("tempdir");
    let (db, ks) = build_and_enrich(tmp.path());
    let stdout = run_rule(
        &db,
        ks,
        &rule("canonical-bypass-dead.cypher"),
        PARAMS_BYPASS,
    );

    assert!(
        stdout.contains("record_orphan"),
        "record_orphan calls the bypass append() and is NOT reached from any \
         :EntryPoint → must surface as BYPASS_DEAD (reproducing qbot-core \
         #3544/#3545/#3546 scatter shape):\n{stdout}"
    );
    assert!(
        !stdout.contains("record_trade\""),
        "record_trade IS reached via cli::run_record → must NOT surface as \
         BYPASS_DEAD (it is BYPASS_REACHABLE):\n{stdout}"
    );
    assert!(
        !stdout.contains("seed_fixture"),
        "seed_fixture is test-only → is_test filter MUST drop it:\n{stdout}"
    );
}

#[test]
fn canonical_unreachable_rule_reproduces_1526() {
    let tmp = tempdir().expect("tempdir");
    let (db, ks) = build_and_enrich(tmp.path());
    let stdout = run_rule(
        &db,
        ks,
        &rule("canonical-unreachable.cypher"),
        PARAMS_UNREACHABLE,
    );

    assert!(
        stdout.contains("record_isolated"),
        "record_isolated lives in the canonical crate (CANONICAL_FOR edge) and \
         is NOT reached from any :EntryPoint → must surface as \
         CANONICAL_UNREACHABLE (reproducing qbot-core #1526 shape):\n{stdout}"
    );
    assert!(
        !stdout.contains("record_trade_safe"),
        "record_trade_safe is reached via cli::run_record → must NOT surface \
         as CANONICAL_UNREACHABLE:\n{stdout}"
    );
}

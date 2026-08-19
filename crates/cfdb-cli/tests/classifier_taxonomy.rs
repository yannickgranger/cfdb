#![cfg(all(feature = "hir", feature = "classify"))]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use assert_cmd::prelude::*;

mod common;

fn queries_dir() -> PathBuf {
    let cfdb_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("cfdb-cli crate dir has a parent crates/")
        .parent()
        .expect("crates/ has a parent cfdb sub-workspace root");
    cfdb_root.join("examples/queries")
}

fn fixture_dir() -> PathBuf {
    queries_dir().join("fixtures/classifier-taxonomy")
}

fn cfdb() -> Command {
    Command::cargo_bin("cfdb").expect("cfdb binary is built for integration tests")
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

fn shared_cls_db() -> PathBuf {
    common::cached_db("classifier-taxonomy-cls", |db| {
        let workspace = db.join("_workspace");
        copy_dir_recursive(&fixture_dir(), &workspace);
        common::extract(db, &workspace, "cls", &["--hir", "--no-proc-macro"]);

        cfdb()
            .args([
                "enrich-concepts",
                "--db",
                db.to_str().expect("db utf-8"),
                "--keyspace",
                "cls",
                "--workspace",
                workspace.to_str().expect("workspace utf-8"),
            ])
            .assert()
            .success();

        cfdb()
            .args([
                "enrich-reachability",
                "--db",
                db.to_str().expect("db utf-8"),
                "--keyspace",
                "cls",
            ])
            .assert()
            .success();
    })
}

fn run_scope(db: &Path, ks: &str, context: &str) -> serde_json::Value {
    let out = cfdb()
        .args([
            "scope",
            "--db",
            db.to_str().expect("db utf-8"),
            "--keyspace",
            ks,
            "--context",
            context,
        ])
        .output()
        .expect("spawn cfdb scope");
    assert!(
        out.status.success(),
        "cfdb scope failed: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).expect("scope stdout is JSON")
}

fn bucket<'a>(inv: &'a serde_json::Value, class: &str) -> &'a serde_json::Value {
    inv.get("findings_by_class")
        .and_then(|m| m.get(class))
        .unwrap_or_else(|| panic!("missing findings_by_class.{class}"))
}

fn qnames(bucket: &serde_json::Value) -> Vec<String> {
    bucket
        .as_array()
        .unwrap_or_else(|| panic!("bucket is not an array: {bucket:?}"))
        .iter()
        .filter_map(|row| row.get("qname").and_then(|v| v.as_str()).map(String::from))
        .collect()
}

#[test]
fn classifier_emits_duplicated_feature_for_orderbook_pair() {
    let db = shared_cls_db();
    let ks = "cls";
    let inv = run_scope(&db, ks, "trading");
    let names = qnames(bucket(&inv, "duplicated_feature"));
    assert!(
        names
            .iter()
            .any(|q| q.contains("trading_domain_a") && q.ends_with("OrderBook")),
        "expected trading_domain_a::OrderBook in duplicated_feature, got {names:?}"
    );
    assert!(
        names
            .iter()
            .any(|q| q.contains("trading_domain_b") && q.ends_with("OrderBook")),
        "expected trading_domain_b::OrderBook in duplicated_feature, got {names:?}"
    );
}

#[test]
fn classifier_emits_unfinished_refactor_for_deprecated_item() {
    let db = shared_cls_db();
    let ks = "cls";
    let inv = run_scope(&db, ks, "trading");
    let names = qnames(bucket(&inv, "unfinished_refactor"));
    assert!(
        names.iter().any(|q| q.contains("OldSizer")),
        "expected an OldSizer-rooted item in unfinished_refactor, got {names:?}"
    );
}

#[test]
fn classifier_emits_context_homonym_for_position_value_pair() {
    let db = shared_cls_db();
    let ks = "cls";
    let inv = run_scope(&db, ks, "trading");
    let names = qnames(bucket(&inv, "context_homonym"));
    assert!(
        names
            .iter()
            .any(|q| q.contains("trading_domain_a") && q.contains("value")),
        "expected Position::value from trading_domain_a in context_homonym, got {names:?}"
    );
}

#[test]
fn classifier_emits_random_scattering_for_compute_qty_fork() {
    let db = shared_cls_db();
    let ks = "cls";
    let inv = run_scope(&db, ks, "trading");
    let names = qnames(bucket(&inv, "random_scattering"));
    assert!(
        names.iter().any(|q| q.contains("compute_qty_from_bps")),
        "expected compute_qty_from_bps in random_scattering, got {names:?}"
    );
}

#[test]
fn classifier_emits_canonical_bypass_for_orphan_isolated() {
    let db = shared_cls_db();
    let ks = "cls";
    let inv = run_scope(&db, ks, "trading");
    let names = qnames(bucket(&inv, "canonical_bypass"));
    assert!(
        names
            .iter()
            .any(|q| q.contains("Orphan") || q.contains("isolated")),
        "expected an Orphan::isolated-rooted item in canonical_bypass, got {names:?}"
    );
}

#[test]
fn classifier_emits_unwired_for_dead_function() {
    let db = shared_cls_db();
    let ks = "cls";
    let inv = run_scope(&db, ks, "trading");
    let names = qnames(bucket(&inv, "unwired"));
    assert!(
        names.iter().any(|q| q.contains("dead_function")),
        "expected dead_function in unwired, got {names:?}"
    );
}

#[test]
fn classifier_six_buckets_all_populated() {
    let db = shared_cls_db();
    let ks = "cls";
    let inv = run_scope(&db, ks, "trading");
    for class in [
        "duplicated_feature",
        "context_homonym",
        "unfinished_refactor",
        "random_scattering",
        "canonical_bypass",
        "unwired",
    ] {
        let rows = bucket(&inv, class)
            .as_array()
            .unwrap_or_else(|| panic!("bucket {class} not an array"));
        assert!(
            !rows.is_empty(),
            "classifier bucket `{class}` is empty — expected ≥1 finding on the \
             classifier-taxonomy fixture. Buckets: {}",
            serde_json::to_string_pretty(&inv["findings_by_class"]).unwrap_or_default()
        );
    }
}

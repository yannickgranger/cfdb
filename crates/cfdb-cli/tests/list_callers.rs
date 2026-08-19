use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use assert_cmd::prelude::*;
use tempfile::tempdir;

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("mkdir -p");
    }
    fs::write(path, contents).expect("write fixture file");
}

fn build_fixture_workspace(root: &Path) -> PathBuf {
    write(
        &root.join("Cargo.toml"),
        r#"[workspace]
resolver = "2"
members = ["filters", "consumer"]
"#,
    );

    write(
        &root.join("filters/Cargo.toml"),
        r#"[package]
name = "filters"
version = "0.0.1"
edition = "2021"
publish = false

[lib]
path = "src/lib.rs"
"#,
    );
    write(
        &root.join("filters/src/lib.rs"),
        r#"pub struct KalmanFilter;

impl KalmanFilter {
    pub fn new() -> Self { KalmanFilter }
    pub fn apply_kalman(&self, _x: f64) -> f64 { 0.0 }
}

pub fn kalman_smooth(_series: &[f64]) -> Vec<f64> { Vec::new() }
"#,
    );

    write(
        &root.join("consumer/Cargo.toml"),
        r#"[package]
name = "consumer"
version = "0.0.1"
edition = "2021"
publish = false

[lib]
path = "src/lib.rs"
"#,
    );
    write(
        &root.join("consumer/src/lib.rs"),
        r#"// Three call sites that should be found by cfdb list-callers:
// 1. Path call to `KalmanFilter::new` (associated fn).
// 2. Path call to `kalman_smooth` (free fn).
// 3. Method call to `apply_kalman` on a KalmanFilter instance.
// Plus one fn that never touches kalman — MUST NOT appear in results.

pub fn build_filter() {
    let _f = KalmanFilter::new();
}

pub fn smooth_prices(prices: &[f64]) -> Vec<f64> {
    kalman_smooth(prices)
}

pub fn apply_to_series(f: &KalmanFilter, x: f64) -> f64 {
    f.apply_kalman(x)
}

pub fn not_a_kalman_user(a: i64, b: i64) -> i64 {
    a + b
}

// The text fixtures pretend these exist in scope; syn is text-only so
// the consumer code compiles against its own declarations here. The
// extractor doesn't do type resolution.
pub struct KalmanFilter;
impl KalmanFilter {
    pub fn new() -> Self { KalmanFilter }
    pub fn apply_kalman(&self, _x: f64) -> f64 { 0.0 }
}
pub fn kalman_smooth(_series: &[f64]) -> Vec<f64> { Vec::new() }
"#,
    );

    root.to_path_buf()
}

fn fresh_fixture_keyspace() -> (tempfile::TempDir, tempfile::TempDir) {
    let fixture = tempdir().expect("fixture tempdir");
    let workspace = build_fixture_workspace(fixture.path());
    let db = tempdir().expect("db tempdir");

    Command::cargo_bin("cfdb")
        .expect("cfdb binary is built for integration tests")
        .args([
            "extract",
            "--workspace",
            workspace
                .to_str()
                .expect("fixture workspace tempdir path is valid utf-8"),
            "--db",
            db.path().to_str().expect("db tempdir path is valid utf-8"),
            "--keyspace",
            "fixture",
        ])
        .assert()
        .success();

    (fixture, db)
}

fn run_list_callers(db_path: &Path, qname: &str) -> String {
    let output = Command::cargo_bin("cfdb")
        .expect("cfdb binary is built for integration tests")
        .args([
            "list-callers",
            "--db",
            db_path.to_str().expect("db tempdir path is valid utf-8"),
            "--keyspace",
            "fixture",
            "--qname",
            qname,
        ])
        .output()
        .expect("run cfdb list-callers");
    assert!(
        output.status.success(),
        "cfdb list-callers failed (qname={qname:?}): {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn list_callers_template_path() -> PathBuf {
    let cfdb_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("cfdb-cli manifest dir has a parent crates/ directory")
        .parent()
        .expect("crates/ has a parent cfdb sub-workspace root");
    cfdb_root.join("examples/queries/list-callers.cypher")
}

#[test]
fn list_callers_typed_verb_finds_all_three_kalman_call_sites() {
    let (_fixture, db) = fresh_fixture_keyspace();
    let stdout = run_list_callers(db.path(), "(?i).*kalman.*");

    assert!(
        stdout.contains("build_filter"),
        "expected build_filter (path call to KalmanFilter::new) in results:\n{stdout}"
    );
    assert!(
        stdout.contains("smooth_prices"),
        "expected smooth_prices (path call to kalman_smooth) in results:\n{stdout}"
    );
    assert!(
        stdout.contains("apply_to_series"),
        "expected apply_to_series (method call to .apply_kalman) in results:\n{stdout}"
    );

    assert!(
        !stdout.contains("not_a_kalman_user"),
        "not_a_kalman_user leaked into kalman-caller results:\n{stdout}"
    );

    assert!(
        stdout.contains("KalmanFilter::new"),
        "expected KalmanFilter::new callee path in results:\n{stdout}"
    );
    assert!(
        stdout.contains("kalman_smooth"),
        "expected kalman_smooth callee path in results:\n{stdout}"
    );
    assert!(
        stdout.contains("apply_kalman"),
        "expected apply_kalman (method-call last segment) in results:\n{stdout}"
    );
}

#[test]
fn list_callers_typed_verb_filters_by_qname_pattern() {
    let (_fixture, db) = fresh_fixture_keyspace();

    let only_new = run_list_callers(db.path(), "^KalmanFilter::new$");
    assert!(
        only_new.contains("build_filter"),
        "qname=^KalmanFilter::new$ should find build_filter:\n{only_new}"
    );
    assert!(
        !only_new.contains("smooth_prices"),
        "qname=^KalmanFilter::new$ must NOT find smooth_prices:\n{only_new}"
    );
    assert!(
        !only_new.contains("apply_to_series"),
        "qname=^KalmanFilter::new$ must NOT find apply_to_series:\n{only_new}"
    );

    let only_smooth = run_list_callers(db.path(), "^kalman_smooth$");
    assert!(
        only_smooth.contains("smooth_prices"),
        "qname=^kalman_smooth$ should find smooth_prices:\n{only_smooth}"
    );
    assert!(
        !only_smooth.contains("build_filter"),
        "qname=^kalman_smooth$ must NOT find build_filter:\n{only_smooth}"
    );
    assert!(
        !only_smooth.contains("apply_to_series"),
        "qname=^kalman_smooth$ must NOT find apply_to_series:\n{only_smooth}"
    );

    let only_apply = run_list_callers(db.path(), "^apply_kalman$");
    assert!(
        only_apply.contains("apply_to_series"),
        "qname=^apply_kalman$ should find apply_to_series:\n{only_apply}"
    );
    assert!(
        !only_apply.contains("build_filter"),
        "qname=^apply_kalman$ must NOT find build_filter:\n{only_apply}"
    );
    assert!(
        !only_apply.contains("smooth_prices"),
        "qname=^apply_kalman$ must NOT find smooth_prices:\n{only_apply}"
    );
}

#[test]
fn list_callers_typed_verb_equals_raw_query_with_params() {
    let (_fixture, db) = fresh_fixture_keyspace();

    let pattern = "(?i).*kalman.*";

    let typed_stdout = run_list_callers(db.path(), pattern);

    let cypher = fs::read_to_string(list_callers_template_path())
        .expect("read list-callers.cypher template");
    let params_json = format!(r#"{{"qname":"{pattern}"}}"#);
    let raw_output = Command::cargo_bin("cfdb")
        .expect("cfdb binary is built for integration tests")
        .args([
            "query",
            "--db",
            db.path().to_str().expect("db tempdir path is valid utf-8"),
            "--keyspace",
            "fixture",
            "--params",
            &params_json,
            &cypher,
        ])
        .output()
        .expect("run cfdb query --params");
    assert!(
        raw_output.status.success(),
        "cfdb query --params failed: {}",
        String::from_utf8_lossy(&raw_output.stderr)
    );
    let raw_stdout = String::from_utf8_lossy(&raw_output.stdout).into_owned();

    for needle in [
        "build_filter",
        "smooth_prices",
        "apply_to_series",
        "KalmanFilter::new",
        "kalman_smooth",
        "apply_kalman",
    ] {
        assert!(
            typed_stdout.contains(needle),
            "typed-verb output missing {needle}:\n{typed_stdout}"
        );
        assert!(
            raw_stdout.contains(needle),
            "raw-query --params output missing {needle}:\n{raw_stdout}"
        );
    }
    assert!(
        !typed_stdout.contains("not_a_kalman_user"),
        "typed-verb output leaked not_a_kalman_user:\n{typed_stdout}"
    );
    assert!(
        !raw_stdout.contains("not_a_kalman_user"),
        "raw-query output leaked not_a_kalman_user:\n{raw_stdout}"
    );
}

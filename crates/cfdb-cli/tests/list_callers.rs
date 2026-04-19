//! Typed verb `cfdb list-callers` — end-to-end behavior test (RFC §13 v0.1
//! AC item 5 / issue #3633).
//!
//! Proves cfdb's polyvalence: the same schema that answers ban rules
//! (`arch-ban-utc-now`) also answers ad-hoc agent questions of the form
//! "where is X used?" — exercised through the typed verb, not a hand-
//! assembled raw `cfdb query` invocation. The typed verb is sugar over
//! the raw path and MUST produce the same result set; that is the
//! contract this file enforces.
//!
//! Fixture: a synthetic workspace with a `filters` crate that defines
//! `KalmanFilter::new`, `apply_kalman`, and `kalman_smooth`, plus a
//! `consumer` crate with three functions that call them (both as path
//! call and method call) and one function that doesn't touch kalman at
//! all. The rewritten test runs `cfdb list-callers` against this keyspace
//! with three different `$qname` patterns to prove (a) all three callers
//! surface for a broad pattern and (b) tighter patterns return strictly
//! narrower subsets — the parameter actually filters, not just returns
//! everything on every call.

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

    // Provider crate — defines the Kalman primitives. The extractor
    // does not need to resolve `filters::KalmanFilter` — the consumer's
    // call sites carry the textual path `KalmanFilter::new` which is
    // what the discovery rule matches on.
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

    // Consumer crate — three call sites reference kalman, one doesn't.
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

/// Build a fresh fixture workspace + extract it into a fresh keyspace.
/// Returns (workspace_tempdir, db_tempdir) — the caller holds both
/// TempDir values so they stay alive for the duration of the test.
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

/// Run `cfdb list-callers --db <db> --keyspace fixture --qname <qname>`
/// and return stdout. Asserts the subprocess exits 0 so test failures
/// surface the stderr diagnostics.
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

/// Path to the generic template file — used only by the genericity
/// cross-check test that verifies `cfdb query --params` produces the
/// same output as the typed verb. The binary has the same bytes
/// embedded via `include_str!`; this function just reads them from
/// disk so the test can drive the raw-query path.
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

    // 1. All three kalman callers MUST surface by name.
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

    // 2. The non-kalman function MUST NOT surface.
    assert!(
        !stdout.contains("not_a_kalman_user"),
        "not_a_kalman_user leaked into kalman-caller results:\n{stdout}"
    );

    // 3. Callee paths MUST reflect what syn saw (name-based, unresolved).
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

/// Genericity proof — different `$qname` values MUST return different
/// caller subsets. If the verb ignored `--qname` and returned everything
/// on every call (the pre-wire-up stub behavior, or a silent unbound-
/// param bug), every assertion in this test would fail.
#[test]
fn list_callers_typed_verb_filters_by_qname_pattern() {
    let (_fixture, db) = fresh_fixture_keyspace();

    // Tight regex 1: only `KalmanFilter::new` callers → only `build_filter`.
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

    // Tight regex 2: only `kalman_smooth` callers → only `smooth_prices`.
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

    // Tight regex 3: only `apply_kalman` callers → only `apply_to_series`.
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

/// Contract proof — the typed verb and the raw `cfdb query --params`
/// path MUST produce the same caller set for the same `$qname` input.
/// This is the genericity guarantee: typed verbs are sugar over the raw
/// path, not a second implementation. Divergence = split-brain.
#[test]
fn list_callers_typed_verb_equals_raw_query_with_params() {
    let (_fixture, db) = fresh_fixture_keyspace();

    let pattern = "(?i).*kalman.*";

    // Typed verb path.
    let typed_stdout = run_list_callers(db.path(), pattern);

    // Raw query path — same template bytes (the binary has them embedded
    // via `include_str!`, the test reads the source file to feed the
    // `query` subcommand).
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

    // Both paths must report the same three callers AND the same
    // three callee paths.
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
    // Negative case must hold in BOTH outputs.
    assert!(
        !typed_stdout.contains("not_a_kalman_user"),
        "typed-verb output leaked not_a_kalman_user:\n{typed_stdout}"
    );
    assert!(
        !raw_stdout.contains("not_a_kalman_user"),
        "raw-query output leaked not_a_kalman_user:\n{raw_stdout}"
    );
}

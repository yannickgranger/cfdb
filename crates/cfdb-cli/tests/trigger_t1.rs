//! Issue #101 — `cfdb check --trigger T1` integration test (AC-4, AC-5).
//!
//! Builds a synthetic Cargo workspace that declares TWO concept TOMLs
//! exercising two of T1's three sub-verdicts (MISSING_CANONICAL_CRATE
//! and STALE_RFC_REFERENCE), extracts it into a keyspace, runs
//! `enrich-rfc-docs`, then invokes `cfdb check --trigger T1` and asserts
//! exactly 2 findings + exit code 1. The `--no-fail` flag flips exit to
//! 0 without changing the row count.
//!
//! A clean-fixture variant with zero drift asserts exit 0 + empty rows.
//!
//! The test uses the real `cfdb` binary via `Command::cargo_bin("cfdb")`
//! — no mocks, no `InMemoryStore` — so it exercises the same path a
//! user would run from a shell. This matches the template established
//! by `arch_ban_utc_now.rs`.

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

/// Build a synthetic Cargo workspace with two member crates plus two
/// `.cfdb/concepts/*.toml` files wired to trigger exactly two T1
/// sub-verdicts:
///   - `drift-missing-crate.toml`  → MISSING_CANONICAL_CRATE
///     (owns `crate-a`, names a canonical crate `nonexistent-crate` that
///     no workspace member matches; no `owning_rfc` → STALE does not
///     fire; `crate-a` has items so CONCEPT_UNWIRED does not fire)
///   - `drift-stale-rfc.toml`      → STALE_RFC_REFERENCE
///     (owns `crate-b`, canonical crate matches `crate-b`, `owning_rfc`
///     names `RFC-999999-nonexistent` which no `:RfcDoc.path` or
///     `:RfcDoc.title` contains)
///
/// Both contexts own a crate with items so neither fires CONCEPT_UNWIRED.
/// An empty `docs/` layout keeps `enrich-rfc-docs` fast — it still runs
/// and produces zero `:RfcDoc` nodes, which the check verb recognises
/// via a stderr warning.
fn build_drift_fixture(root: &Path) -> PathBuf {
    write(
        &root.join("Cargo.toml"),
        r#"[workspace]
resolver = "2"
members = ["crate-a", "crate-b"]
"#,
    );

    write(
        &root.join("crate-a/Cargo.toml"),
        r#"[package]
name = "crate-a"
version = "0.0.1"
edition = "2021"
publish = false

[lib]
path = "src/lib.rs"
"#,
    );
    write(
        &root.join("crate-a/src/lib.rs"),
        r#"pub fn drift_a_item() -> i64 { 1 }
pub struct DriftAType;
"#,
    );

    write(
        &root.join("crate-b/Cargo.toml"),
        r#"[package]
name = "crate-b"
version = "0.0.1"
edition = "2021"
publish = false

[lib]
path = "src/lib.rs"
"#,
    );
    write(
        &root.join("crate-b/src/lib.rs"),
        r#"pub fn drift_b_item() -> i64 { 2 }
pub struct DriftBType;
"#,
    );

    // TOML 1 — MISSING_CANONICAL_CRATE trip.
    write(
        &root.join(".cfdb/concepts/drift-missing-crate.toml"),
        r#"name = "drift-missing-crate"
canonical_crate = "nonexistent-crate"
crates = ["crate-a"]
"#,
    );

    // TOML 2 — STALE_RFC_REFERENCE trip.
    write(
        &root.join(".cfdb/concepts/drift-stale-rfc.toml"),
        r#"name = "drift-stale-rfc"
canonical_crate = "crate-b"
owning_rfc = "RFC-999999-nonexistent"
crates = ["crate-b"]
"#,
    );

    root.to_path_buf()
}

/// Build a zero-drift fixture: one context whose `canonical_crate`
/// resolves to a real workspace crate, whose `owning_rfc` matches a
/// docs file that `enrich-rfc-docs` indexes, and whose declared crate
/// is non-empty — all three sub-verdicts pass.
fn build_clean_fixture(root: &Path) -> PathBuf {
    write(
        &root.join("Cargo.toml"),
        r#"[workspace]
resolver = "2"
members = ["crate-a"]
"#,
    );

    write(
        &root.join("crate-a/Cargo.toml"),
        r#"[package]
name = "crate-a"
version = "0.0.1"
edition = "2021"
publish = false

[lib]
path = "src/lib.rs"
"#,
    );
    write(
        &root.join("crate-a/src/lib.rs"),
        r#"pub fn clean_item() -> i64 { 3 }
"#,
    );

    // Docs file whose first heading contains the RFC tag the TOML
    // declares. `enrich-rfc-docs` picks it up as a `:RfcDoc` node
    // with `title` that matches the owning_rfc regex.
    write(
        &root.join("docs/RFC-CLEAN-FIXTURE.md"),
        "# RFC-CLEAN-FIXTURE — test doc\n\nclean_item referenced here.\n",
    );

    write(
        &root.join(".cfdb/concepts/clean.toml"),
        r#"name = "clean"
canonical_crate = "crate-a"
owning_rfc = "RFC-CLEAN-FIXTURE"
crates = ["crate-a"]
"#,
    );

    root.to_path_buf()
}

fn run_extract_and_enrich(db: &Path, workspace: &Path, keyspace: &str) {
    Command::cargo_bin("cfdb")
        .expect("cfdb binary is built for integration tests")
        .args([
            "extract",
            "--workspace",
            workspace.to_str().expect("utf-8 path"),
            "--db",
            db.to_str().expect("utf-8 path"),
            "--keyspace",
            keyspace,
        ])
        .assert()
        .success();

    Command::cargo_bin("cfdb")
        .expect("cfdb binary is built for integration tests")
        .args([
            "enrich-rfc-docs",
            "--db",
            db.to_str().expect("utf-8 path"),
            "--keyspace",
            keyspace,
            "--workspace",
            workspace.to_str().expect("utf-8 path"),
        ])
        .assert()
        .success();
}

#[test]
fn t1_drift_fixture_reports_exactly_two_findings_and_exits_one() {
    let fixture = tempdir().expect("fixture tempdir");
    let workspace = build_drift_fixture(fixture.path());

    let db = tempdir().expect("db tempdir");
    run_extract_and_enrich(db.path(), &workspace, "t1-drift");

    // Default: exit 1 when findings fire.
    let output = Command::cargo_bin("cfdb")
        .expect("cfdb binary is built for integration tests")
        .args([
            "check",
            "--trigger",
            "T1",
            "--db",
            db.path().to_str().expect("utf-8 path"),
            "--keyspace",
            "t1-drift",
        ])
        .output()
        .expect("run cfdb check");

    assert!(
        !output.status.success(),
        "`cfdb check --trigger T1` must exit non-zero when findings fire; stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value =
        serde_json::from_str(&stdout).expect("stdout must be well-formed JSON");
    let rows = json.get("rows").and_then(|r| r.as_array()).expect("rows");

    assert_eq!(
        rows.len(),
        2,
        "expected exactly 2 T1 findings on the drift fixture, got {}:\n{stdout}",
        rows.len()
    );

    // Distinct verdicts: one MISSING_CANONICAL_CRATE, one STALE_RFC_REFERENCE.
    let verdicts: Vec<&str> = rows
        .iter()
        .filter_map(|r| r.get("verdict").and_then(|v| v.as_str()))
        .collect();
    assert!(
        verdicts.contains(&"MISSING_CANONICAL_CRATE"),
        "missing MISSING_CANONICAL_CRATE verdict in {verdicts:?}"
    );
    assert!(
        verdicts.contains(&"STALE_RFC_REFERENCE"),
        "missing STALE_RFC_REFERENCE verdict in {verdicts:?}"
    );

    // Evidence carries the offending props.
    assert!(
        stdout.contains("nonexistent-crate"),
        "expected offending canonical_crate evidence in stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("RFC-999999-nonexistent"),
        "expected offending owning_rfc evidence in stdout:\n{stdout}"
    );

    // Stderr summary line is the shared violations-style format.
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("violations: 2 (rule: trigger T1)"),
        "expected `violations: 2 (rule: trigger T1)` on stderr, got:\n{stderr}"
    );
}

#[test]
fn t1_drift_fixture_with_no_fail_exits_zero_but_reports_same_findings() {
    let fixture = tempdir().expect("fixture tempdir");
    let workspace = build_drift_fixture(fixture.path());

    let db = tempdir().expect("db tempdir");
    run_extract_and_enrich(db.path(), &workspace, "t1-drift-nofail");

    // `--no-fail` flips exit to 0 without changing the payload.
    let output = Command::cargo_bin("cfdb")
        .expect("cfdb binary is built for integration tests")
        .args([
            "check",
            "--trigger",
            "T1",
            "--db",
            db.path().to_str().expect("utf-8 path"),
            "--keyspace",
            "t1-drift-nofail",
            "--no-fail",
        ])
        .output()
        .expect("run cfdb check --no-fail");

    assert!(
        output.status.success(),
        "`--no-fail` must exit 0 even with findings; stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value =
        serde_json::from_str(&stdout).expect("stdout must be well-formed JSON");
    let rows = json.get("rows").and_then(|r| r.as_array()).expect("rows");
    assert_eq!(
        rows.len(),
        2,
        "`--no-fail` must not suppress findings in the payload:\n{stdout}"
    );
}

#[test]
fn t1_clean_fixture_reports_zero_findings_and_exits_zero() {
    let fixture = tempdir().expect("fixture tempdir");
    let workspace = build_clean_fixture(fixture.path());

    let db = tempdir().expect("db tempdir");
    run_extract_and_enrich(db.path(), &workspace, "t1-clean");

    let output = Command::cargo_bin("cfdb")
        .expect("cfdb binary is built for integration tests")
        .args([
            "check",
            "--trigger",
            "T1",
            "--db",
            db.path().to_str().expect("utf-8 path"),
            "--keyspace",
            "t1-clean",
        ])
        .output()
        .expect("run cfdb check");

    assert!(
        output.status.success(),
        "clean fixture must exit 0; stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value =
        serde_json::from_str(&stdout).expect("stdout must be well-formed JSON");
    let rows = json.get("rows").and_then(|r| r.as_array()).expect("rows");
    assert!(
        rows.is_empty(),
        "clean fixture must report zero T1 findings, got {}:\n{stdout}",
        rows.len()
    );

    // Stderr summary reports zero.
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("violations: 0 (rule: trigger T1)"),
        "expected `violations: 0` summary, got:\n{stderr}"
    );
}

#[test]
fn t1_unknown_trigger_is_rejected_with_derived_valid_values() {
    let db = tempdir().expect("db tempdir");
    // Build a trivial keyspace so the verb reaches the trigger parse
    // path. Even a stub keyspace is enough — clap rejects the
    // --trigger arg before the verb body runs.
    let fixture = tempdir().expect("fixture tempdir");
    let workspace = build_clean_fixture(fixture.path());
    run_extract_and_enrich(db.path(), &workspace, "t1-unknown");

    let output = Command::cargo_bin("cfdb")
        .expect("cfdb binary is built for integration tests")
        .args([
            "check",
            "--trigger",
            "T9999",
            "--db",
            db.path().to_str().expect("utf-8 path"),
            "--keyspace",
            "t1-unknown",
        ])
        .output()
        .expect("run cfdb check with bogus trigger");

    assert!(
        !output.status.success(),
        "bogus `--trigger T9999` must fail clap value parsing"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("valid values: T1"),
        "error message must enumerate valid trigger ids (anti-regression: global CLAUDE.md §7 \
         MCP/CLI boundary fix AC); got:\n{stderr}"
    );
}

//! Exit-code contract integration tests for the `cfdb` CLI binary.
//!
//! RFC-044 §3.6 (slice 044-F) — tests the four documented codes:
//!
//! - `0`  — success / no findings (clean run or `--no-fail`)
//! - `1`  — runtime error (handler returned `Err`)
//! - `2`  — usage error (bad CLI arguments, clap parse failure)
//! - `30` — findings present (rule rows returned without `--no-fail`)
//!
//! These tests complement `exit_codes_violations.rs` (which covers the
//! `violations` sub-command in depth). This file focuses on the contract
//! across multiple sub-commands and on the 1/2 boundary that
//! `exit_codes_violations.rs` covers only for `violations`.
//!
//! Fixture pattern: reuse the same `build_minimal_workspace` + helpers
//! as `exit_codes_violations.rs` but inline here to keep the two files
//! independent (RFC-044 I1: no shared infrastructure across slices).

use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use tempfile::tempdir;

// ---------------------------------------------------------------------------
// Fixture helpers
// ---------------------------------------------------------------------------

fn write_file(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("mkdir -p");
    }
    fs::write(path, contents).expect("write fixture file");
}

/// One-crate workspace with a pub fn whose name a ban rule can match.
fn build_minimal_workspace(root: &Path) -> PathBuf {
    write_file(
        &root.join("Cargo.toml"),
        r#"[workspace]
resolver = "2"
members = ["only-crate"]
"#,
    );
    write_file(
        &root.join("only-crate/Cargo.toml"),
        r#"[package]
name = "only-crate"
version = "0.0.1"
edition = "2021"
publish = false

[lib]
path = "src/lib.rs"
"#,
    );
    write_file(
        &root.join("only-crate/src/lib.rs"),
        r#"pub fn flagged_item() -> i64 {
    42
}
"#,
    );
    root.to_path_buf()
}

/// Extract the workspace into a fresh db directory, asserting success.
fn extract_into(workspace: &Path, db: &Path, keyspace: &str) {
    Command::cargo_bin("cfdb")
        .expect("cfdb binary must be built for integration tests")
        .args([
            "extract",
            "--workspace",
            workspace.to_str().expect("utf-8 workspace path"),
            "--db",
            db.to_str().expect("utf-8 db path"),
            "--keyspace",
            keyspace,
        ])
        .assert()
        .success();
}

/// Write a Cypher rule file and return its path.
fn write_rule(dir: &Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, body).expect("write rule file");
    path
}

// ---------------------------------------------------------------------------
// Exit code 0 — success / no findings
// ---------------------------------------------------------------------------

/// `cfdb violations` with a rule that returns zero rows exits 0.
///
/// Verifies the "clean run" half of the contract. The `violations`
/// sub-command is the canonical path that would exit 30 if rows were
/// present — confirming it exits 0 when there are none gives confidence
/// that the centralization of `findings_exit()` did not break the
/// zero-row happy path.
#[test]
fn clean_violations_run_exits_0() {
    let fixture = tempdir().expect("fixture tempdir");
    let workspace = build_minimal_workspace(fixture.path());
    let db = tempdir().expect("db tempdir");
    extract_into(&workspace, db.path(), "ks-clean");

    let rule_dir = tempdir().expect("rule tempdir");
    let rule = write_rule(
        rule_dir.path(),
        "zero.cypher",
        r#"MATCH (i:Item) WHERE i.name = "this_name_does_not_exist" RETURN i.qname AS qname"#,
    );

    let output = Command::cargo_bin("cfdb")
        .expect("cfdb binary must be built for integration tests")
        .args([
            "violations",
            "--db",
            db.path().to_str().expect("utf-8 db path"),
            "--keyspace",
            "ks-clean",
            "--rule",
            rule.to_str().expect("utf-8 rule path"),
        ])
        .output()
        .expect("run cfdb violations");

    assert_eq!(
        output.status.code(),
        Some(0),
        "zero-row violation rule must exit 0; stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

// ---------------------------------------------------------------------------
// Exit code 1 — runtime error (handler returned Err)
// ---------------------------------------------------------------------------

/// `cfdb violations` with a missing keyspace JSON exits 1 (runtime error),
/// not 30 (findings).
///
/// The db directory is empty (never extracted into), so loading the keyspace
/// fails with a `StoreError` → `CfdbCliError::Store` → exit 1.
#[test]
fn missing_keyspace_exits_1() {
    let db = tempdir().expect("db tempdir");
    // Intentionally skip extract_into — db is empty, keyspace load will fail.
    let rule_dir = tempdir().expect("rule tempdir");
    let rule = write_rule(
        rule_dir.path(),
        "any.cypher",
        r#"MATCH (i:Item) RETURN i.qname AS qname"#,
    );

    let output = Command::cargo_bin("cfdb")
        .expect("cfdb binary must be built for integration tests")
        .args([
            "violations",
            "--db",
            db.path().to_str().expect("utf-8 db path"),
            "--keyspace",
            "ghost-keyspace",
            "--rule",
            rule.to_str().expect("utf-8 rule path"),
        ])
        .output()
        .expect("run cfdb violations against missing keyspace");

    assert_eq!(
        output.status.code(),
        Some(1),
        "missing keyspace must exit 1 (runtime error), not 30 (rule hits); \
         stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// `cfdb dump` against a missing keyspace exits 1 (runtime error).
///
/// Exercises a different sub-command on the runtime-error path to confirm
/// that `exit_code_for` is wired for all handler `Err` returns, not just
/// the `violations` arm. The db directory exists but the keyspace was never
/// extracted, so `dump` hits a `StoreError` → exit 1.
#[test]
fn dump_missing_keyspace_exits_1() {
    let db = tempdir().expect("db tempdir");
    // Intentionally skip extract_into — db directory exists but keyspace
    // JSON file is absent, so `dump` returns a runtime error.

    let output = Command::cargo_bin("cfdb")
        .expect("cfdb binary must be built for integration tests")
        .args([
            "dump",
            "--db",
            db.path().to_str().expect("utf-8 db path"),
            "--keyspace",
            "ghost-keyspace-for-dump",
        ])
        .output()
        .expect("run cfdb dump against missing keyspace");

    assert_eq!(
        output.status.code(),
        Some(1),
        "dump with missing keyspace must exit 1 (runtime error); \
         stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

// ---------------------------------------------------------------------------
// Exit code 2 — usage error (clap parse failure / bad arguments)
// ---------------------------------------------------------------------------

/// Unknown top-level flag → clap usage error → exit 2.
///
/// The binary's `Cli::parse()` fails before any handler is called; clap
/// writes the error to stderr and exits 2. This path is NOT mediated by
/// `exit_code_for` (it's clap's own exit), but the contract doc says 2 for
/// "clap parse failure" and this test pins that.
#[test]
fn unknown_flag_exits_2() {
    let output = Command::cargo_bin("cfdb")
        .expect("cfdb binary must be built for integration tests")
        .args(["--this-flag-does-not-exist"])
        .output()
        .expect("run cfdb with unknown flag");

    assert_eq!(
        output.status.code(),
        Some(2),
        "unknown flag must exit 2 (clap usage); stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// `cfdb violations` with an unknown sub-command flag → clap usage → exit 2.
///
/// Ensures the 2 code fires at the sub-command level too, not just top-level.
#[test]
fn violations_unknown_flag_exits_2() {
    let output = Command::cargo_bin("cfdb")
        .expect("cfdb binary must be built for integration tests")
        .args(["violations", "--this-flag-does-not-exist", "value"])
        .output()
        .expect("run cfdb violations with bogus flag");

    assert_eq!(
        output.status.code(),
        Some(2),
        "unknown violations flag must exit 2 (clap usage); stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

// ---------------------------------------------------------------------------
// Exit code 30 — findings present
// ---------------------------------------------------------------------------

/// `cfdb violations` with a rule that matches → exit 30.
///
/// Verifies that the centralized `findings_exit()` call in `main_exit.rs`
/// is wired correctly through `dispatch_core` (Violations arm).
#[test]
fn violations_with_findings_exits_30() {
    let fixture = tempdir().expect("fixture tempdir");
    let workspace = build_minimal_workspace(fixture.path());
    let db = tempdir().expect("db tempdir");
    extract_into(&workspace, db.path(), "ks-findings");

    let rule_dir = tempdir().expect("rule tempdir");
    // The fixture pub fn is named `flagged_item`; this rule matches it.
    let rule = write_rule(
        rule_dir.path(),
        "ban.cypher",
        r#"MATCH (i:Item) WHERE i.name = "flagged_item" RETURN i.qname AS qname"#,
    );

    let output = Command::cargo_bin("cfdb")
        .expect("cfdb binary must be built for integration tests")
        .args([
            "violations",
            "--db",
            db.path().to_str().expect("utf-8 db path"),
            "--keyspace",
            "ks-findings",
            "--rule",
            rule.to_str().expect("utf-8 rule path"),
        ])
        .output()
        .expect("run cfdb violations with matching rule");

    assert_eq!(
        output.status.code(),
        Some(30),
        "findings must exit 30 (gate failure); stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// `cfdb violations --rule <missing-file>` exits 2 (usage error), not 1.
///
/// A nonexistent rule path is a user-fixable input mistake: the rule reader
/// maps the `io::Error` to `CfdbCliError::Usage`, which `exit_code_for` maps
/// to 2. Pins the RFC-044 §3.6 self-dogfood "nonexistent rule → 2" assertion.
/// (The issue's `arch-ban-path-regex.cypher` example names a file that does
/// not exist in the repo — a spec typo; this test exercises the same code
/// path directly with an unambiguously-absent path.)
#[test]
fn nonexistent_rule_file_exits_2() {
    let fixture = tempdir().expect("fixture tempdir");
    let workspace = build_minimal_workspace(fixture.path());
    let db = tempdir().expect("db tempdir");
    extract_into(&workspace, db.path(), "ks-missing-rule");

    let output = Command::cargo_bin("cfdb")
        .expect("cfdb binary must be built for integration tests")
        .args([
            "violations",
            "--db",
            db.path().to_str().expect("utf-8 db path"),
            "--keyspace",
            "ks-missing-rule",
            "--rule",
            "/tmp/cfdb-this-rule-does-not-exist.cypher",
        ])
        .output()
        .expect("run cfdb violations with a missing rule file");

    assert_eq!(
        output.status.code(),
        Some(2),
        "a nonexistent --rule path is a usage error (exit 2), not a runtime error (1); \
         stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

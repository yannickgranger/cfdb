//! Exit-code contract for `cfdb violations`. Pinning issue #269 (audit
//! ID CFDB-CLI-H1, EPIC #273): exit code 1 used to collapse runtime
//! errors and rule-row hits, making it impossible for CI scripts (e.g.
//! `ci/cross-dogfood.sh`) to disambiguate "extractor blew up" from
//! "rule found rows." Each test below pins one of the four
//! contractually-distinct codes:
//!
//! - 0  — no findings (clean fixture, rule returns zero rows)
//! - 0  — `--no-fail` overrides findings (rows present, exit suppressed)
//! - 30 — findings present (gate failure, mirrors cross-dogfood.sh)
//! - 1  — runtime error (missing keyspace JSON file → handler `Err`)
//! - 2  — clap usage error (unknown flag)
//!
//! The fixture is intentionally minimal (one crate, one pub fn) so the
//! test stays focused on the exit-code contract rather than rule semantics.

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

/// Build a minimal one-crate workspace with a single `pub fn` named
/// `tagged_for_findings`. The MATCHING rule below greps for that exact
/// name; the NON-MATCHING rule searches for a name that doesn't exist.
fn build_minimal_workspace(root: &Path) -> PathBuf {
    write(
        &root.join("Cargo.toml"),
        r#"[workspace]
resolver = "2"
members = ["only-crate"]
"#,
    );
    write(
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
    write(
        &root.join("only-crate/src/lib.rs"),
        r#"pub fn tagged_for_findings() -> i64 {
    42
}
"#,
    );
    root.to_path_buf()
}

/// Cypher rule that MATCHES `tagged_for_findings` — every row returned
/// counts as a violation. Returning `i.qname` keeps the row shape lint
/// happy (single string-shaped column).
fn matching_rule_cypher() -> &'static str {
    r#"MATCH (i:Item) WHERE i.name = "tagged_for_findings" RETURN i.qname AS qname"#
}

/// Cypher rule that returns ZERO rows on the fixture above (no item with
/// this name exists). Same projection shape as the matching rule.
fn zero_row_rule_cypher() -> &'static str {
    r#"MATCH (i:Item) WHERE i.name = "definitely_not_in_the_fixture" RETURN i.qname AS qname"#
}

fn extract_into(db: &Path, workspace: &Path, keyspace: &str) {
    Command::cargo_bin("cfdb")
        .expect("cfdb binary is built for integration tests")
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

fn write_rule(dir: &Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, body).expect("write rule");
    path
}

/// Synthetic keyspace + rule that returns rows → exit 30.
#[test]
fn violations_with_rows_exits_30() {
    let fixture = tempdir().expect("fixture tempdir");
    let workspace = build_minimal_workspace(fixture.path());
    let db = tempdir().expect("db tempdir");
    extract_into(db.path(), &workspace, "ks-rows");

    let rule_dir = tempdir().expect("rule tempdir");
    let rule = write_rule(rule_dir.path(), "match.cypher", matching_rule_cypher());

    let output = Command::cargo_bin("cfdb")
        .expect("cfdb binary is built for integration tests")
        .args([
            "violations",
            "--db",
            db.path().to_str().expect("utf-8 db path"),
            "--keyspace",
            "ks-rows",
            "--rule",
            rule.to_str().expect("utf-8 rule path"),
        ])
        .output()
        .expect("run cfdb violations");

    assert_eq!(
        output.status.code(),
        Some(30),
        "rule rows present must exit 30 (issue #269); stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Synthetic keyspace + rule that returns 0 rows → exit 0.
#[test]
fn violations_with_zero_rows_exits_0() {
    let fixture = tempdir().expect("fixture tempdir");
    let workspace = build_minimal_workspace(fixture.path());
    let db = tempdir().expect("db tempdir");
    extract_into(db.path(), &workspace, "ks-zero");

    let rule_dir = tempdir().expect("rule tempdir");
    let rule = write_rule(rule_dir.path(), "zero.cypher", zero_row_rule_cypher());

    let output = Command::cargo_bin("cfdb")
        .expect("cfdb binary is built for integration tests")
        .args([
            "violations",
            "--db",
            db.path().to_str().expect("utf-8 db path"),
            "--keyspace",
            "ks-zero",
            "--rule",
            rule.to_str().expect("utf-8 rule path"),
        ])
        .output()
        .expect("run cfdb violations");

    assert_eq!(
        output.status.code(),
        Some(0),
        "zero rows must exit 0; stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// `--no-fail` + matching rule → exit 0 (informational override).
#[test]
fn violations_with_rows_and_no_fail_exits_0() {
    let fixture = tempdir().expect("fixture tempdir");
    let workspace = build_minimal_workspace(fixture.path());
    let db = tempdir().expect("db tempdir");
    extract_into(db.path(), &workspace, "ks-nofail");

    let rule_dir = tempdir().expect("rule tempdir");
    let rule = write_rule(rule_dir.path(), "match.cypher", matching_rule_cypher());

    let output = Command::cargo_bin("cfdb")
        .expect("cfdb binary is built for integration tests")
        .args([
            "violations",
            "--db",
            db.path().to_str().expect("utf-8 db path"),
            "--keyspace",
            "ks-nofail",
            "--rule",
            rule.to_str().expect("utf-8 rule path"),
            "--no-fail",
        ])
        .output()
        .expect("run cfdb violations --no-fail");

    assert_eq!(
        output.status.code(),
        Some(0),
        "--no-fail must exit 0 even with rows; stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Missing keyspace JSON → handler returns Err → exit 1 (runtime error,
/// distinct from exit 30 for "rule rows present").
#[test]
fn violations_missing_keyspace_exits_1() {
    let db = tempdir().expect("db tempdir");
    // Intentionally do NOT call `extract_into`. The `db` directory is
    // empty so loading keyspace `ghost` fails with a runtime error.

    let rule_dir = tempdir().expect("rule tempdir");
    let rule = write_rule(rule_dir.path(), "any.cypher", matching_rule_cypher());

    let output = Command::cargo_bin("cfdb")
        .expect("cfdb binary is built for integration tests")
        .args([
            "violations",
            "--db",
            db.path().to_str().expect("utf-8 db path"),
            "--keyspace",
            "ghost",
            "--rule",
            rule.to_str().expect("utf-8 rule path"),
        ])
        .output()
        .expect("run cfdb violations against missing keyspace");

    assert_eq!(
        output.status.code(),
        Some(1),
        "missing keyspace must exit 1 (runtime error), NOT 30 (rule hits); \
         stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Unknown flag → clap usage error → exit 2 (distinct from runtime
/// error 1 and rule-hits 30).
#[test]
fn violations_unknown_flag_exits_2() {
    let output = Command::cargo_bin("cfdb")
        .expect("cfdb binary is built for integration tests")
        .args(["violations", "--this-flag-does-not-exist", "value"])
        .output()
        .expect("run cfdb violations with bogus flag");

    assert_eq!(
        output.status.code(),
        Some(2),
        "unknown flag must exit 2 (clap usage); stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use tempfile::tempdir;

fn write_file(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("mkdir -p");
    }
    fs::write(path, contents).expect("write fixture file");
}

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

fn write_rule(dir: &Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, body).expect("write rule file");
    path
}

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

#[test]
fn missing_keyspace_exits_1() {
    let db = tempdir().expect("db tempdir");
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

#[test]
fn dump_missing_keyspace_exits_1() {
    let db = tempdir().expect("db tempdir");

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

#[test]
fn violations_with_findings_exits_30() {
    let fixture = tempdir().expect("fixture tempdir");
    let workspace = build_minimal_workspace(fixture.path());
    let db = tempdir().expect("db tempdir");
    extract_into(&workspace, db.path(), "ks-findings");

    let rule_dir = tempdir().expect("rule tempdir");
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

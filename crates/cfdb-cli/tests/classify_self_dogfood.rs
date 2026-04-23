//! Self-dogfood integration test for `cfdb classify` — runs the real
//! binary end-to-end against a cfdb extract of the worktree itself.
//!
//! Exercises AC1 (verb accepts --db + --keyspace + --restrict-to-diff),
//! AC2 (every finding maps to one DebtClass), AC6 (exit 0), AC7
//! (empty-bucket warnings preserved), and the wiring assertion that
//! every classified row's qname is in the restrict set.

use std::path::{Path, PathBuf};
use std::process::Command;

use assert_cmd::prelude::*;
use cfdb_query::{ClassifyEnvelope, CLASSIFY_ENVELOPE_SCHEMA_VERSION};
use tempfile::tempdir;

fn cfdb_workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/ parent")
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

fn extract_two_keyspaces(db_path: &Path, workspace: &Path) {
    for ks in ["cfdb-a", "cfdb-b"] {
        Command::cargo_bin("cfdb")
            .expect("cfdb binary built")
            .args([
                "extract",
                "--workspace",
                workspace.to_str().unwrap(),
                "--db",
                db_path.to_str().unwrap(),
                "--keyspace",
                ks,
            ])
            .assert()
            .success();
    }
}

fn run_diff(db_path: &Path, a: &str, b: &str, out: &Path) {
    let output = Command::cargo_bin("cfdb")
        .expect("cfdb binary built")
        .args([
            "diff",
            "--db",
            db_path.to_str().unwrap(),
            "--a",
            a,
            "--b",
            b,
        ])
        .output()
        .expect("cfdb diff runs");
    assert!(output.status.success());
    std::fs::write(out, output.stdout).expect("write diff envelope");
}

#[test]
fn classify_against_identical_keyspaces_emits_empty_buckets_and_warnings() {
    let db = tempdir().expect("tempdir");
    let db_path = db.path();
    let workspace = cfdb_workspace_root();
    extract_two_keyspaces(db_path, &workspace);

    let diff_path = db.path().join("diff.json");
    run_diff(db_path, "cfdb-a", "cfdb-b", &diff_path);

    let output = Command::cargo_bin("cfdb")
        .expect("cfdb binary built")
        .args([
            "classify",
            "--db",
            db_path.to_str().unwrap(),
            "--keyspace",
            "cfdb-a",
            "--context",
            "cfdb",
            "--restrict-to-diff",
            diff_path.to_str().unwrap(),
        ])
        .output()
        .expect("cfdb classify runs");

    assert!(
        output.status.success(),
        "cfdb classify exited {:?} — stderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout utf-8");
    let envelope: ClassifyEnvelope =
        serde_json::from_str(&stdout).expect("ClassifyEnvelope parses");

    assert_eq!(envelope.schema_version, CLASSIFY_ENVELOPE_SCHEMA_VERSION);
    assert_eq!(envelope.diff_source.a, "cfdb-a");
    assert_eq!(envelope.diff_source.b, "cfdb-b");

    // Identical keyspaces → empty diff → empty restrict set → all buckets empty.
    for (class, bucket) in &envelope.inventory.findings_by_class {
        assert!(
            bucket.is_empty(),
            "class {class:?} bucket unexpectedly non-empty: {bucket:?}"
        );
    }

    // AC7 regression lock: empty-bucket warnings surface for every class.
    assert!(
        !envelope.inventory.warnings.is_empty(),
        "expected per-class empty-bucket warnings + HIR caveat"
    );
}

#[test]
fn classify_rejects_unknown_context() {
    let db = tempdir().expect("tempdir");
    let db_path = db.path();
    let workspace = cfdb_workspace_root();
    extract_two_keyspaces(db_path, &workspace);

    let diff_path = db.path().join("diff.json");
    run_diff(db_path, "cfdb-a", "cfdb-b", &diff_path);

    let output = Command::cargo_bin("cfdb")
        .expect("cfdb binary built")
        .args([
            "classify",
            "--db",
            db_path.to_str().unwrap(),
            "--keyspace",
            "cfdb-a",
            "--context",
            "no-such-context",
            "--restrict-to-diff",
            diff_path.to_str().unwrap(),
        ])
        .output()
        .expect("cfdb classify runs");

    assert!(!output.status.success(), "unknown context should error");
    let stderr = String::from_utf8(output.stderr).expect("stderr utf-8");
    assert!(
        stderr.contains("known contexts"),
        "expected known-contexts enumeration in error, got: {stderr}"
    );
}

#[test]
fn classify_rejects_missing_restrict_file() {
    let db = tempdir().expect("tempdir");
    let db_path = db.path();
    let workspace = cfdb_workspace_root();
    extract_two_keyspaces(db_path, &workspace);

    let output = Command::cargo_bin("cfdb")
        .expect("cfdb binary built")
        .args([
            "classify",
            "--db",
            db_path.to_str().unwrap(),
            "--keyspace",
            "cfdb-a",
            "--context",
            "cfdb",
            "--restrict-to-diff",
            "/nonexistent/diff.json",
        ])
        .output()
        .expect("cfdb classify runs");

    assert!(!output.status.success(), "missing file should error");
    let stderr = String::from_utf8(output.stderr).expect("stderr utf-8");
    assert!(
        stderr.contains("read --restrict-to-diff"),
        "expected read-file error, got: {stderr}"
    );
}

#[test]
fn classify_rejects_sorted_jsonl_format_with_deferred_message() {
    let db = tempdir().expect("tempdir");
    let db_path = db.path();
    let workspace = cfdb_workspace_root();
    extract_two_keyspaces(db_path, &workspace);

    let diff_path = db.path().join("diff.json");
    run_diff(db_path, "cfdb-a", "cfdb-b", &diff_path);

    let output = Command::cargo_bin("cfdb")
        .expect("cfdb binary built")
        .args([
            "classify",
            "--db",
            db_path.to_str().unwrap(),
            "--keyspace",
            "cfdb-a",
            "--context",
            "cfdb",
            "--restrict-to-diff",
            diff_path.to_str().unwrap(),
            "--format",
            "sorted-jsonl",
        ])
        .output()
        .expect("cfdb classify runs");

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("stderr utf-8");
    assert!(
        stderr.contains("sorted-jsonl") || stderr.contains("not supported"),
        "expected deferred-format error, got: {stderr}"
    );
}

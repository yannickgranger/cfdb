//! Integration tests for `cfdb diff` — exercise the CLI handler against a
//! real cfdb binary + real on-disk keyspaces.
//!
//! These cover AC2 (identical keyspaces → empty envelope, exit 0), AC3
//! (determinism across two runs), and the `--format sorted-jsonl` branch.

use std::path::{Path, PathBuf};
use std::process::Command;

use assert_cmd::prelude::*;
use cfdb_query::diff::{DiffEnvelope, ENVELOPE_SCHEMA_VERSION};
use tempfile::tempdir;

fn cfdb_workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("cfdb-cli manifest dir has a parent crates/ directory")
        .parent()
        .expect("crates/ has a parent cfdb workspace root")
        .to_path_buf()
}

fn extract_two_identical_keyspaces(db_path: &Path, workspace: &Path) {
    for ks in ["cfdb-a", "cfdb-b"] {
        Command::cargo_bin("cfdb")
            .expect("cfdb binary built for integration tests")
            .args([
                "extract",
                "--workspace",
                workspace.to_str().expect("workspace utf-8"),
                "--db",
                db_path.to_str().expect("db utf-8"),
                "--keyspace",
                ks,
            ])
            .assert()
            .success();
    }
}

#[test]
fn identical_keyspaces_emit_empty_envelope_and_exit_zero() {
    let db = tempdir().expect("tempdir");
    let db_path = db.path();
    let workspace = cfdb_workspace_root();
    extract_two_identical_keyspaces(db_path, &workspace);

    let output = Command::cargo_bin("cfdb")
        .expect("cfdb binary built for integration tests")
        .args([
            "diff",
            "--db",
            db_path.to_str().expect("db utf-8"),
            "--a",
            "cfdb-a",
            "--b",
            "cfdb-b",
        ])
        .output()
        .expect("cfdb diff runs");

    assert!(
        output.status.success(),
        "cfdb diff exited {:?} — stderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout utf-8");
    let envelope: DiffEnvelope = serde_json::from_str(&stdout).expect("envelope parses");
    assert_eq!(envelope.a, "cfdb-a");
    assert_eq!(envelope.b, "cfdb-b");
    assert_eq!(envelope.schema_version, ENVELOPE_SCHEMA_VERSION);
    assert!(
        envelope.added.is_empty(),
        "added should be empty: {:?}",
        envelope.added
    );
    assert!(
        envelope.removed.is_empty(),
        "removed should be empty: {:?}",
        envelope.removed
    );
    assert!(
        envelope.changed.is_empty(),
        "changed should be empty: {:?}",
        envelope.changed
    );
    assert!(envelope.warnings.is_empty());
}

#[test]
fn back_to_back_runs_are_byte_identical() {
    let db = tempdir().expect("tempdir");
    let db_path = db.path();
    let workspace = cfdb_workspace_root();
    extract_two_identical_keyspaces(db_path, &workspace);

    let run = || {
        Command::cargo_bin("cfdb")
            .expect("cfdb binary built for integration tests")
            .args([
                "diff",
                "--db",
                db_path.to_str().expect("db utf-8"),
                "--a",
                "cfdb-a",
                "--b",
                "cfdb-b",
            ])
            .output()
            .expect("cfdb diff runs")
            .stdout
    };

    assert_eq!(
        run(),
        run(),
        "two back-to-back runs must produce byte-identical stdout"
    );
}

#[test]
fn sorted_jsonl_format_emits_header_line() {
    let db = tempdir().expect("tempdir");
    let db_path = db.path();
    let workspace = cfdb_workspace_root();
    extract_two_identical_keyspaces(db_path, &workspace);

    let output = Command::cargo_bin("cfdb")
        .expect("cfdb binary built for integration tests")
        .args([
            "diff",
            "--db",
            db_path.to_str().expect("db utf-8"),
            "--a",
            "cfdb-a",
            "--b",
            "cfdb-b",
            "--format",
            "sorted-jsonl",
        ])
        .output()
        .expect("cfdb diff runs");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("stdout utf-8");
    let first_line = stdout.lines().next().expect("at least a header line");
    let header: serde_json::Value = serde_json::from_str(first_line).expect("header is JSON");
    assert_eq!(header["op"], "header");
    assert_eq!(header["a"], "cfdb-a");
    assert_eq!(header["b"], "cfdb-b");
    assert_eq!(header["schema_version"], ENVELOPE_SCHEMA_VERSION);
}

#[test]
fn missing_keyspace_errors_with_message() {
    let db = tempdir().expect("tempdir");
    let db_path = db.path();
    let workspace = cfdb_workspace_root();
    extract_two_identical_keyspaces(db_path, &workspace);

    let output = Command::cargo_bin("cfdb")
        .expect("cfdb binary built for integration tests")
        .args([
            "diff",
            "--db",
            db_path.to_str().expect("db utf-8"),
            "--a",
            "nope",
            "--b",
            "cfdb-b",
        ])
        .output()
        .expect("cfdb diff runs");

    assert!(!output.status.success(), "missing keyspace should error");
    let stderr = String::from_utf8(output.stderr).expect("stderr utf-8");
    assert!(
        stderr.contains("keyspace `nope` not found"),
        "expected missing-keyspace error, got: {stderr}"
    );
}

#[test]
fn unknown_kind_token_errors_cleanly() {
    let db = tempdir().expect("tempdir");
    let db_path = db.path();
    let workspace = cfdb_workspace_root();
    extract_two_identical_keyspaces(db_path, &workspace);

    let output = Command::cargo_bin("cfdb")
        .expect("cfdb binary built for integration tests")
        .args([
            "diff",
            "--db",
            db_path.to_str().expect("db utf-8"),
            "--a",
            "cfdb-a",
            "--b",
            "cfdb-b",
            "--kinds",
            "Item",
        ])
        .output()
        .expect("cfdb diff runs");

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("stderr utf-8");
    assert!(
        stderr.contains("unknown kind"),
        "expected unknown-kind error, got: {stderr}"
    );
}

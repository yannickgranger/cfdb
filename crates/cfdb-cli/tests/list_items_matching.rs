use std::path::{Path, PathBuf};
use std::process::Command;

use assert_cmd::prelude::*;
use tempfile::tempdir;

fn cfdb_workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("cfdb-cli manifest dir has a parent crates/ directory")
        .parent()
        .expect("crates/ has a parent cfdb sub-workspace root")
        .to_path_buf()
}

fn extract_cfdb(db_path: &Path) {
    Command::cargo_bin("cfdb")
        .expect("cfdb binary is built for integration tests")
        .args([
            "extract",
            "--workspace",
            cfdb_workspace_root()
                .to_str()
                .expect("cfdb sub-workspace root path is valid utf-8"),
            "--db",
            db_path.to_str().expect("db tempdir path is valid utf-8"),
            "--keyspace",
            "cfdb-v01",
        ])
        .assert()
        .success();
}

#[test]
fn list_items_matching_help_lists_every_flag() {
    let out = Command::cargo_bin("cfdb")
        .expect("cfdb binary is built for integration tests")
        .args(["list-items-matching", "--help"])
        .output()
        .expect("spawn `cfdb list-items-matching --help`");
    assert!(
        out.status.success(),
        "--help must exit 0; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    for flag in [
        "--db",
        "--keyspace",
        "--name-pattern",
        "--kinds",
        "--group-by-context",
    ] {
        assert!(
            stdout.contains(flag),
            "help for list-items-matching missing flag `{flag}`:\n{stdout}"
        );
    }
}

#[test]
fn list_items_matching_rejects_unknown_kind_with_exit_2() {
    let out = Command::cargo_bin("cfdb")
        .expect("cfdb binary is built for integration tests")
        .args([
            "list-items-matching",
            "--db",
            "/does-not-matter",
            "--keyspace",
            "cfdb-v01",
            "--name-pattern",
            ".*",
            "--kinds",
            "NotAKind",
        ])
        .output()
        .expect("spawn cfdb list-items-matching");
    assert_eq!(
        out.status.code(),
        Some(2),
        "clap value-parser rejection must exit code 2"
    );
}

#[test]
fn list_items_matching_filters_keyspace_by_name_pattern() {
    let db = tempdir().expect("tempdir");
    extract_cfdb(db.path());

    let out = Command::cargo_bin("cfdb")
        .expect("cfdb binary is built for integration tests")
        .args([
            "list-items-matching",
            "--db",
            db.path().to_str().expect("tempdir is utf-8"),
            "--keyspace",
            "cfdb-v01",
            "--name-pattern",
            "^StoreBackend$",
        ])
        .output()
        .expect("spawn cfdb list-items-matching");
    assert!(
        out.status.success(),
        "list-items-matching failed: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("output is QueryResult JSON");
    let rows = parsed["rows"].as_array().expect("rows array");
    assert!(
        !rows.is_empty(),
        "expected at least one row for ^StoreBackend$: {stdout}"
    );
    let row = &rows[0];
    assert_eq!(
        row["name"].as_str(),
        Some("StoreBackend"),
        "row.name must be StoreBackend: {row}"
    );
    assert_eq!(
        row["kind"].as_str(),
        Some("trait"),
        "row.kind must be extractor-lowercase \"trait\" for StoreBackend: {row}"
    );
    for col in [
        "qname",
        "name",
        "kind",
        "crate",
        "file",
        "line",
        "bounded_context",
    ] {
        assert!(
            row.get(col).is_some(),
            "row is missing required column `{col}`: {row}"
        );
    }
}

#[test]
fn list_items_matching_subsumes_list_definitions_of_when_kinds_omitted() {
    let db = tempdir().expect("tempdir");
    extract_cfdb(db.path());

    let out = Command::cargo_bin("cfdb")
        .expect("cfdb binary is built for integration tests")
        .args([
            "list-items-matching",
            "--db",
            db.path().to_str().expect("utf-8"),
            "--keyspace",
            "cfdb-v01",
            "--name-pattern",
            "^Query$",
        ])
        .output()
        .expect("spawn cfdb");
    assert!(out.status.success());

    let parsed: serde_json::Value = serde_json::from_slice(&out.stdout).expect("JSON");
    let rows = parsed["rows"].as_array().expect("rows");
    let query_struct_row = rows
        .iter()
        .find(|r| r["name"].as_str() == Some("Query") && r["kind"].as_str() == Some("struct"));
    assert!(
        query_struct_row.is_some(),
        "expected a row with name=Query and kind=struct, got rows: {rows:?}"
    );
}

#[test]
fn list_items_matching_filters_by_kinds_at_cli_surface() {
    let db = tempdir().expect("tempdir");
    extract_cfdb(db.path());

    let out = Command::cargo_bin("cfdb")
        .expect("cfdb binary is built for integration tests")
        .args([
            "list-items-matching",
            "--db",
            db.path().to_str().expect("utf-8"),
            "--keyspace",
            "cfdb-v01",
            "--name-pattern",
            ".*",
            "--kinds",
            "Trait",
        ])
        .output()
        .expect("spawn cfdb");
    assert!(out.status.success());

    let parsed: serde_json::Value = serde_json::from_slice(&out.stdout).expect("JSON");
    let rows = parsed["rows"].as_array().expect("rows");
    assert!(
        !rows.is_empty(),
        "cfdb workspace has known traits (StoreBackend, Visit, ...); expected non-empty rows"
    );
    for (idx, row) in rows.iter().enumerate() {
        assert_eq!(
            row["kind"].as_str(),
            Some("trait"),
            "row {idx} does not carry kind=trait: {row}"
        );
    }
}

#[test]
fn list_items_matching_group_by_context_partitions_real_keyspace() {
    let db = tempdir().expect("tempdir");
    extract_cfdb(db.path());

    let out = Command::cargo_bin("cfdb")
        .expect("cfdb binary is built for integration tests")
        .args([
            "list-items-matching",
            "--db",
            db.path().to_str().expect("utf-8"),
            "--keyspace",
            "cfdb-v01",
            "--name-pattern",
            "^Query$|^QueryBuilder$|^Store.*",
            "--kinds",
            "Struct,Trait",
            "--group-by-context",
        ])
        .output()
        .expect("spawn cfdb");
    assert!(
        out.status.success(),
        "group-by-context run failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let parsed: serde_json::Value = serde_json::from_slice(&out.stdout).expect("JSON");
    let rows = parsed["rows"].as_array().expect("rows");
    assert!(
        !rows.is_empty(),
        "expected at least one bounded_context partition row"
    );
    let mut saw_items_list = false;
    for row in rows {
        let obj = row.as_object().expect("row is object");
        assert!(
            obj.contains_key("bounded_context"),
            "row missing bounded_context column: {row}"
        );
        if let Some(items) = obj.get("items").and_then(|v| v.as_array()) {
            if !items.is_empty() {
                saw_items_list = true;
            }
        }
    }
    assert!(
        saw_items_list,
        "expected at least one partition with a non-empty items list: {parsed:?}"
    );
}

#[test]
fn list_items_matching_warns_on_unemitted_impl_block() {
    let db = tempdir().expect("tempdir");
    extract_cfdb(db.path());

    let out = Command::cargo_bin("cfdb")
        .expect("cfdb binary is built for integration tests")
        .args([
            "list-items-matching",
            "--db",
            db.path().to_str().expect("utf-8"),
            "--keyspace",
            "cfdb-v01",
            "--name-pattern",
            ".*",
            "--kinds",
            "ImplBlock",
        ])
        .output()
        .expect("spawn cfdb");
    assert!(out.status.success());

    let parsed: serde_json::Value = serde_json::from_slice(&out.stdout).expect("JSON");
    let warnings = parsed["warnings"].as_array().expect("warnings array");
    let combined = serde_json::to_string(warnings).expect("ser warnings");
    assert!(
        combined.contains("ImplBlock") && combined.contains("not emitted"),
        "expected ImplBlock-not-emitted warning, got: {combined}"
    );
}

#[test]
fn list_items_matching_deterministic_across_runs_at_cli_surface() {
    let db = tempdir().expect("tempdir");
    extract_cfdb(db.path());

    let run = || {
        Command::cargo_bin("cfdb")
            .expect("cfdb bin")
            .args([
                "list-items-matching",
                "--db",
                db.path().to_str().expect("utf-8"),
                "--keyspace",
                "cfdb-v01",
                "--name-pattern",
                "^Pattern$|^Predicate$|^Expr$",
                "--kinds",
                "Struct,Enum",
            ])
            .output()
            .expect("spawn cfdb")
    };
    let a = run();
    let b = run();
    assert!(a.status.success() && b.status.success());
    assert_eq!(
        a.stdout, b.stdout,
        "two runs with identical args must produce identical stdout (G1 determinism)"
    );
}

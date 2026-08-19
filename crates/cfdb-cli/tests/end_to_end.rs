use std::path::{Path, PathBuf};
use std::process::Command;

use assert_cmd::prelude::*;
use tempfile::tempdir;

fn cfdb_workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("cfdb-cli manifest dir has a parent crates/ directory")
        .parent()
        .expect("crates/ has a parent cfdb workspace root")
        .to_path_buf()
}

#[test]
fn end_to_end_extract_then_query_finds_store_backend_trait() {
    let db = tempdir().expect("tempdir");
    let db_path = db.path();
    let workspace = cfdb_workspace_root();

    Command::cargo_bin("cfdb")
        .expect("cfdb binary is built for integration tests")
        .args([
            "extract",
            "--workspace",
            workspace
                .to_str()
                .expect("cfdb sub-workspace root path is valid utf-8"),
            "--db",
            db_path.to_str().expect("db tempdir path is valid utf-8"),
            "--keyspace",
            "cfdb-v01",
        ])
        .assert()
        .success();

    let list = Command::cargo_bin("cfdb")
        .expect("cfdb binary is built for integration tests")
        .args([
            "list-keyspaces",
            "--db",
            db_path.to_str().expect("db tempdir path is valid utf-8"),
        ])
        .output()
        .expect("spawn `cfdb list-keyspaces`");
    assert!(list.status.success());
    let stdout = String::from_utf8_lossy(&list.stdout);
    assert!(
        stdout.lines().any(|l| l == "cfdb-v01"),
        "keyspace missing in list: {stdout}"
    );

    let query_out = Command::cargo_bin("cfdb")
        .expect("cfdb binary is built for integration tests")
        .args([
            "query",
            "--db",
            db_path.to_str().expect("db tempdir path is valid utf-8"),
            "--keyspace",
            "cfdb-v01",
            "MATCH (i:Item) WHERE i.name = 'StoreBackend' RETURN i.qname, i.kind",
        ])
        .output()
        .expect("spawn `cfdb query`");
    assert!(
        query_out.status.success(),
        "query failed: stderr={}",
        String::from_utf8_lossy(&query_out.stderr)
    );
    let json = String::from_utf8_lossy(&query_out.stdout);
    assert!(
        json.contains("StoreBackend"),
        "expected StoreBackend in query output, got: {json}"
    );
    assert!(
        json.contains("\"trait\""),
        "expected kind=trait in query output, got: {json}"
    );

    let count_out = Command::cargo_bin("cfdb")
        .expect("cfdb binary is built for integration tests")
        .args([
            "query",
            "--db",
            db_path.to_str().expect("db tempdir path is valid utf-8"),
            "--keyspace",
            "cfdb-v01",
            "MATCH (i:Item) WHERE i.crate = 'cfdb-query' RETURN count(*) AS n",
        ])
        .output()
        .expect("spawn `cfdb query`");
    assert!(count_out.status.success());
    let json = String::from_utf8_lossy(&count_out.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
    let n = parsed["rows"][0]["n"].as_i64().expect("n is integer");
    assert!(n >= 1, "expected at least 1 cfdb-query item, got {n}");
}

#[test]
fn dump_output_is_pure_jsonl_sorted_by_label_qname() {
    let db = tempdir().expect("tempdir");
    let db_path = db.path();
    let workspace = cfdb_workspace_root();

    Command::cargo_bin("cfdb")
        .expect("cfdb binary is built for integration tests")
        .args([
            "extract",
            "--workspace",
            workspace
                .to_str()
                .expect("cfdb sub-workspace root path is valid utf-8"),
            "--db",
            db_path.to_str().expect("db tempdir path is valid utf-8"),
            "--keyspace",
            "cfdb-v01",
        ])
        .assert()
        .success();

    let dump_out = Command::cargo_bin("cfdb")
        .expect("cfdb binary is built for integration tests")
        .args([
            "dump",
            "--db",
            db_path.to_str().expect("db tempdir path is valid utf-8"),
            "--keyspace",
            "cfdb-v01",
        ])
        .output()
        .expect("spawn `cfdb dump`");
    assert!(
        dump_out.status.success(),
        "dump failed: stderr={}",
        String::from_utf8_lossy(&dump_out.stderr)
    );

    let stdout = String::from_utf8(dump_out.stdout).expect("dump output is UTF-8");
    let body = stdout.strip_suffix('\n').unwrap_or(&stdout);
    assert!(!body.is_empty(), "dump produced no output");

    let mut node_count = 0usize;
    let mut edge_count = 0usize;
    let mut last_node_sort: Option<(String, String)> = None;
    let mut last_edge_sort: Option<(String, String, String)> = None;
    for line in body.lines() {
        assert!(
            !line.starts_with("N\t") && !line.starts_with("E\t"),
            "line uses banned tab-prefix format: {line}"
        );
        let v: serde_json::Value = serde_json::from_str(line)
            .unwrap_or_else(|e| panic!("line is not pure JSON: {line:?}: {e}"));
        let obj = v.as_object().expect("dump line must be a JSON object");

        let kind = obj
            .get("kind")
            .and_then(|k| k.as_str())
            .expect("every line must carry a `kind` discriminator");
        match kind {
            "node" => {
                node_count += 1;
                let label = obj
                    .get("label")
                    .and_then(|l| l.as_str())
                    .expect("node line must have label")
                    .to_string();
                let qname = obj
                    .get("props")
                    .and_then(|p| p.get("qname"))
                    .and_then(|q| q.as_str())
                    .map(String::from)
                    .or_else(|| obj.get("id").and_then(|i| i.as_str()).map(String::from))
                    .expect("node line must have qname or id for sort fallback");
                let key = (label, qname);
                if let Some(prev) = &last_node_sort {
                    assert!(
                        prev <= &key,
                        "node sort violation: {prev:?} appeared before {key:?}"
                    );
                }
                last_node_sort = Some(key);
            }
            "edge" => {
                edge_count += 1;
                let label = obj
                    .get("label")
                    .and_then(|l| l.as_str())
                    .expect("edge line must have label")
                    .to_string();
                let src_qname = obj
                    .get("src_qname")
                    .and_then(|s| s.as_str())
                    .expect("edge line must have src_qname")
                    .to_string();
                let dst_qname = obj
                    .get("dst_qname")
                    .and_then(|s| s.as_str())
                    .expect("edge line must have dst_qname")
                    .to_string();
                let key = (label, src_qname, dst_qname);
                if let Some(prev) = &last_edge_sort {
                    assert!(
                        prev <= &key,
                        "edge sort violation: {prev:?} appeared before {key:?}"
                    );
                }
                last_edge_sort = Some(key);
            }
            other => panic!("unknown kind discriminator: {other}"),
        }
    }
    assert!(node_count > 0, "expected at least one node line in dump");
    assert!(edge_count > 0, "expected at least one edge line in dump");
}

#[test]
fn two_extractions_produce_byte_identical_dump() {
    use std::process::Stdio;

    let db_a = tempdir().expect("tempdir-a");
    let db_b = tempdir().expect("tempdir-b");
    let workspace = cfdb_workspace_root();

    for db in [db_a.path(), db_b.path()] {
        Command::cargo_bin("cfdb")
            .expect("cfdb binary is built for integration tests")
            .args([
                "extract",
                "--workspace",
                workspace
                    .to_str()
                    .expect("cfdb sub-workspace root path is valid utf-8"),
                "--db",
                db.to_str().expect("db tempdir path is valid utf-8"),
                "--keyspace",
                "cfdb-v01",
            ])
            .assert()
            .success();
    }

    fn dump_bytes(db: &Path) -> Vec<u8> {
        let out = Command::cargo_bin("cfdb")
            .expect("cfdb binary is built for integration tests")
            .args([
                "dump",
                "--db",
                db.to_str().expect("db tempdir path is valid utf-8"),
                "--keyspace",
                "cfdb-v01",
            ])
            .stderr(Stdio::null())
            .output()
            .expect("spawn `cfdb dump`");
        assert!(out.status.success());
        out.stdout
    }

    let bytes_a = dump_bytes(db_a.path());
    let bytes_b = dump_bytes(db_b.path());
    assert_eq!(
        bytes_a, bytes_b,
        "G1: two independent extractions of the same workspace must produce \
         byte-identical dump output"
    );
}

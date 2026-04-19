//! End-to-end integration test — RFC §13 felt win.
//!
//! Runs the `cfdb` binary against the cfdb sub-workspace itself:
//! 1. `cfdb extract --workspace <cfdb> --db <tempdir>`
//! 2. `cfdb list-keyspaces --db <tempdir>` → includes `cfdb-v01`
//! 3. `cfdb query ... 'MATCH (i:Item) WHERE i.name = "StoreBackend" ...'`
//!    → finds the `StoreBackend` trait defined in `cfdb-core`
//! 4. `cfdb query ... 'MATCH (i:Item) WHERE i.crate = "cfdb-query" ...'`
//!    → finds at least 1 item (QueryBuilder or similar)
//!
//! This is the v0.1 acceptance gate: cfdb can index a real Rust workspace
//! and answer a real structural question via the CLI wire form.

use std::path::{Path, PathBuf};
use std::process::Command;

use assert_cmd::prelude::*;
use tempfile::tempdir;

fn cfdb_workspace_root() -> PathBuf {
    // This test binary lives at .concept-graph/cfdb/target/debug/deps/...;
    // CARGO_MANIFEST_DIR for this crate is .concept-graph/cfdb/crates/cfdb-cli/.
    // The cfdb sub-workspace root is two levels up.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("cfdb-cli manifest dir has a parent crates/ directory")
        .parent()
        .expect("crates/ has a parent cfdb sub-workspace root")
        .to_path_buf()
}

#[test]
fn end_to_end_extract_then_query_finds_store_backend_trait() {
    let db = tempdir().expect("tempdir");
    let db_path = db.path();
    let workspace = cfdb_workspace_root();

    // 1. Extract cfdb itself.
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

    // 2. List keyspaces includes cfdb-v01.
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

    // 3. Query for the StoreBackend trait by name.
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

    // 4. Count items in cfdb-query crate — should find at least QueryBuilder.
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
    // Parse the JSON to extract the count.
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
    let n = parsed["rows"][0]["n"].as_i64().expect("n is integer");
    assert!(n >= 1, "expected at least 1 cfdb-query item, got {n}");
}

/// `cfdb dump` end-to-end (#3630): extract a real workspace, dump it, assert
/// every line is pure JSONL with alphabetical-key envelopes and the §12.1
/// sort key. Guards against the OLD tab-prefixed `N\t...\t{json}` shape.
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

    // `cfdb dump` uses println! which appends a trailing LF — strip exactly one.
    let stdout = String::from_utf8(dump_out.stdout).expect("dump output is UTF-8");
    let body = stdout.strip_suffix('\n').unwrap_or(&stdout);
    assert!(!body.is_empty(), "dump produced no output");

    // Every line MUST be a pure JSON object — no tab-prefix, no positional cols.
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

/// Two consecutive `cfdb extract` + `cfdb dump` runs on the SAME workspace
/// MUST produce byte-identical sha256. This is the Gate 1 / §12.1 G1
/// observable behavior — what the CI harness `determinism-check.sh` checks.
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

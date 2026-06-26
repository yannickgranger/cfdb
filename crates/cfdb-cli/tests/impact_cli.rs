//! `cfdb impact` end-to-end — RFC-047 slice 47-B (#490).
//!
//! Drives the real `cfdb` binary against a fact-injected fixture keyspace
//! (option-2 "integration against real inputs"): items in known files joined by
//! a `CALLS` chain. Proves the whole verb path — load → seed resolution →
//! `impact_query` → emit — for both `--item` (direct seeds) and `--since`
//! (seeds from `git diff --name-only`).

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

use cfdb_core::fact::{Edge, Node, PropValue};
use cfdb_core::qname::item_node_id;
use cfdb_core::schema::{EdgeLabel, Keyspace, Label};
use cfdb_core::store::StoreBackend;
use cfdb_petgraph::{persist, PetgraphStore};

fn item(qname: &str, file: &str) -> Node {
    let mut props = BTreeMap::new();
    props.insert("qname".into(), PropValue::Str(qname.into()));
    props.insert("file".into(), PropValue::Str(file.into()));
    props.insert("kind".into(), PropValue::Str("fn".into()));
    Node {
        id: item_node_id(qname),
        label: Label::new(Label::ITEM),
        props,
    }
}

fn calls(caller: &str, callee: &str) -> Edge {
    Edge {
        src: item_node_id(caller),
        dst: item_node_id(callee),
        label: EdgeLabel::new(EdgeLabel::CALLS),
        props: BTreeMap::new(),
    }
}

/// `x::top ─CALLS▶ x::mid ─CALLS▶ x::leaf`, plus an unconnected `x::island`.
/// Reverse-reachable callers of `x::leaf` are {`x::mid`, `x::top`}.
fn write_fixture_keyspace(db_dir: &Path) {
    let ks = Keyspace::new("imp");
    let nodes = vec![
        item("x::leaf", "crates/x/leaf.rs"),
        item("x::mid", "crates/x/mid.rs"),
        item("x::top", "crates/x/top.rs"),
        item("x::island", "crates/x/island.rs"),
    ];
    let edges = vec![calls("x::mid", "x::leaf"), calls("x::top", "x::mid")];
    let mut store = PetgraphStore::new();
    store
        .ingest_nodes(&ks, nodes)
        .expect("ingest fixture nodes");
    store
        .ingest_edges(&ks, edges)
        .expect("ingest fixture edges");
    persist::save(&store, &ks, &db_dir.join("imp.json")).expect("persist fixture keyspace");
}

fn cfdb() -> Command {
    Command::new(env!("CARGO_BIN_EXE_cfdb"))
}

#[test]
fn impact_item_returns_transitive_callers() {
    let db = tempfile::tempdir().expect("db tempdir");
    write_fixture_keyspace(db.path());

    let out = cfdb()
        .args(["impact", "--keyspace", "imp", "--item", "x::leaf"])
        .arg("--db")
        .arg(db.path())
        .output()
        .expect("run cfdb impact");

    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("x::mid"),
        "mid is a depth-1 caller: {stdout}"
    );
    assert!(
        stdout.contains("x::top"),
        "top is a depth-2 caller: {stdout}"
    );
    assert!(
        !stdout.contains("x::island"),
        "an item with no CALLS path to the seed must not appear: {stdout}"
    );
}

#[test]
fn impact_max_depth_bounds_the_traversal() {
    let db = tempfile::tempdir().expect("db tempdir");
    write_fixture_keyspace(db.path());

    // `--max-depth 1` from `x::leaf` keeps the depth-1 caller `x::mid` but drops
    // the depth-2 caller `x::top` (RFC-047a §6 — maps to `CALLS*1..1`).
    let out = cfdb()
        .args([
            "impact",
            "--keyspace",
            "imp",
            "--item",
            "x::leaf",
            "--max-depth",
            "1",
        ])
        .arg("--db")
        .arg(db.path())
        .output()
        .expect("run cfdb impact --max-depth");

    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("x::mid"),
        "depth-1 caller must be present: {stdout}"
    );
    assert!(
        !stdout.contains("x::top"),
        "depth-2 caller must be excluded by --max-depth 1: {stdout}"
    );
}

#[test]
fn impact_since_resolves_seeds_from_git_diff() {
    let db = tempfile::tempdir().expect("db tempdir");
    write_fixture_keyspace(db.path());

    // A throwaway git repo whose last commit changes `crates/x/leaf.rs` — the
    // file that defines the seed. `git diff --name-only HEAD~1..HEAD` reports
    // it, so `--since HEAD~1` must seed `x::leaf` and return its callers.
    let ws = tempfile::tempdir().expect("ws tempdir");
    let git = |args: &[&str]| {
        let ok = Command::new("git")
            .arg("-C")
            .arg(ws.path())
            .args(args)
            .output()
            .expect("git")
            .status
            .success();
        assert!(ok, "git {args:?} failed");
    };
    git(&["init", "-q"]);
    git(&["config", "user.email", "t@example.com"]);
    git(&["config", "user.name", "t"]);
    git(&["commit", "--allow-empty", "-q", "-m", "base"]);
    std::fs::create_dir_all(ws.path().join("crates/x")).expect("mkdir");
    std::fs::write(ws.path().join("crates/x/leaf.rs"), "fn leaf() {}\n").expect("write");
    git(&["add", "."]);
    git(&["commit", "-q", "-m", "change leaf"]);

    let out = cfdb()
        .args(["impact", "--keyspace", "imp", "--since", "HEAD~1"])
        .arg("--db")
        .arg(db.path())
        .arg("--workspace")
        .arg(ws.path())
        .output()
        .expect("run cfdb impact --since");

    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("x::mid") && stdout.contains("x::top"),
        "--since must seed x::leaf (from the changed file) and return its callers: {stdout}"
    );
}

#[test]
fn impact_requires_item_or_since() {
    let db = tempfile::tempdir().expect("db tempdir");
    write_fixture_keyspace(db.path());

    let out = cfdb()
        .args(["impact", "--keyspace", "imp"])
        .arg("--db")
        .arg(db.path())
        .output()
        .expect("run cfdb impact");

    assert!(!out.status.success(), "no --item/--since must be an error");
}

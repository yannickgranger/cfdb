//! Issue #102 — `cfdb check --trigger T3` integration test (AC-4 + AC-5).
//!
//! Builds TWO synthetic Cargo workspace fixtures exercising the two
//! `is_cross_context` branches:
//!
//!   - `same-context` fixture: 2 crates in one bounded context
//!     (`alpha`), both defining `pub struct OrderStatus` →
//!     exactly 1 T3 row with `is_cross_context = false`.
//!   - `cross-context` fixture: 2 crates in different bounded contexts
//!     (`alpha` + `beta`), both defining `pub struct OrderStatus` →
//!     exactly 1 T3 row with `is_cross_context = true`.
//!
//! Each fixture is extracted via the real `cfdb` binary and the T3
//! trigger is run against the resulting keyspace. Assertions
//! verify the row count, the `is_cross_context` flag, and the
//! `canonical_candidate` derivation.
//!
//! Template follows `arch_ban_utc_now.rs` + `trigger_t1.rs` —
//! `Command::cargo_bin("cfdb")` + tempdir + real keyspace. No mocks.

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

/// Shared helper — builds a Cargo workspace with two library crates,
/// both defining `pub struct OrderStatus` so T3's same-name-multi-crate
/// detector fires on one row. Caller supplies the `.cfdb/concepts/*.toml`
/// contents so the bounded-context assignment drives the
/// `is_cross_context` branch under test.
fn build_workspace_with_shared_name(root: &Path, concept_tomls: &[(&str, &str)]) -> PathBuf {
    write(
        &root.join("Cargo.toml"),
        r#"[workspace]
resolver = "2"
members = ["crate-a", "crate-b"]
"#,
    );

    for name in ["crate-a", "crate-b"] {
        write(
            &root.join(format!("{name}/Cargo.toml")),
            &format!(
                r#"[package]
name = "{name}"
version = "0.0.1"
edition = "2021"
publish = false

[lib]
path = "src/lib.rs"
"#
            ),
        );
        // Same struct name in both crates — the T3 signal.
        write(
            &root.join(format!("{name}/src/lib.rs")),
            "pub struct OrderStatus;\n",
        );
    }

    for (filename, body) in concept_tomls {
        write(&root.join(format!(".cfdb/concepts/{filename}")), body);
    }

    root.to_path_buf()
}

fn build_same_context_fixture(root: &Path) -> PathBuf {
    build_workspace_with_shared_name(
        root,
        &[(
            "alpha.toml",
            r#"name = "alpha"
canonical_crate = "crate-a"
crates = ["crate-a", "crate-b"]
"#,
        )],
    )
}

fn build_cross_context_fixture(root: &Path) -> PathBuf {
    build_workspace_with_shared_name(
        root,
        &[
            (
                "alpha.toml",
                r#"name = "alpha"
canonical_crate = "crate-a"
crates = ["crate-a"]
"#,
            ),
            (
                "beta.toml",
                r#"name = "beta"
crates = ["crate-b"]
"#,
            ),
        ],
    )
}

fn run_extract(db: &Path, workspace: &Path, keyspace: &str) {
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
}

fn run_check_t3(db: &Path, keyspace: &str, no_fail: bool) -> std::process::Output {
    let mut args = vec![
        "check",
        "--trigger",
        "T3",
        "--db",
        db.to_str().expect("utf-8 path"),
        "--keyspace",
        keyspace,
    ];
    if no_fail {
        args.push("--no-fail");
    }
    Command::cargo_bin("cfdb")
        .expect("cfdb binary is built for integration tests")
        .args(args)
        .output()
        .expect("run cfdb check --trigger T3")
}

#[test]
fn t3_same_context_fixture_reports_one_row_with_is_cross_context_false() {
    let fixture = tempdir().expect("fixture tempdir");
    let workspace = build_same_context_fixture(fixture.path());

    let db = tempdir().expect("db tempdir");
    run_extract(db.path(), &workspace, "t3-same");

    // Default exit: 1 when ANY T3 row fires. The same-context dup IS
    // a finding even though it's not a cross-context one — T3 is the
    // raw detector; the classifier decides what to do with same-
    // context candidates.
    let output = run_check_t3(db.path(), "t3-same", false);
    assert!(
        !output.status.success(),
        "T3 must exit 1 on any raw Pattern A row; stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("well-formed JSON");
    let rows = json.get("rows").and_then(|r| r.as_array()).expect("rows");

    assert_eq!(
        rows.len(),
        1,
        "same-context fixture has exactly one shared name (OrderStatus); got:\n{stdout}"
    );

    let row = &rows[0];
    assert_eq!(
        row.get("name").and_then(|v| v.as_str()),
        Some("OrderStatus")
    );
    assert_eq!(row.get("kind").and_then(|v| v.as_str()), Some("struct"));
    assert_eq!(
        row.get("is_cross_context").and_then(|v| v.as_bool()),
        Some(false),
        "same-context fixture must report is_cross_context=false; row:\n{row:#}"
    );
    assert_eq!(
        row.get("n_contexts").and_then(|v| v.as_i64()),
        Some(1),
        "same-context fixture must report n_contexts=1; row:\n{row:#}"
    );
    assert_eq!(
        row.get("canonical_candidate").and_then(|v| v.as_str()),
        Some("crate-a"),
        "canonical_candidate must resolve to the context's canonical_crate; row:\n{row:#}"
    );
}

#[test]
fn t3_cross_context_fixture_reports_is_cross_context_true() {
    let fixture = tempdir().expect("fixture tempdir");
    let workspace = build_cross_context_fixture(fixture.path());

    let db = tempdir().expect("db tempdir");
    run_extract(db.path(), &workspace, "t3-cross");

    let output = run_check_t3(db.path(), "t3-cross", false);
    assert!(
        !output.status.success(),
        "T3 must exit 1 on cross-context row; stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("well-formed JSON");
    let rows = json.get("rows").and_then(|r| r.as_array()).expect("rows");

    assert_eq!(
        rows.len(),
        1,
        "cross-context fixture has exactly one shared name; got:\n{stdout}"
    );

    let row = &rows[0];
    assert_eq!(
        row.get("name").and_then(|v| v.as_str()),
        Some("OrderStatus")
    );
    assert_eq!(
        row.get("is_cross_context").and_then(|v| v.as_bool()),
        Some(true),
        "cross-context fixture must report is_cross_context=true; row:\n{row:#}"
    );
    assert_eq!(
        row.get("n_contexts").and_then(|v| v.as_i64()),
        Some(2),
        "cross-context fixture must report n_contexts=2; row:\n{row:#}"
    );

    // bounded_contexts[] carries both contexts, sorted by cypher's
    // `collect(DISTINCT ...)` emission (BTreeMap-backed → lexicographic).
    let contexts = row
        .get("bounded_contexts")
        .and_then(|v| v.as_array())
        .expect("bounded_contexts array");
    let contexts_str: Vec<&str> = contexts.iter().filter_map(|v| v.as_str()).collect();
    assert_eq!(
        contexts_str,
        vec!["alpha", "beta"],
        "bounded_contexts must contain both alpha and beta; row:\n{row:#}"
    );

    // The crate-a context has `canonical_crate = "crate-a"`; beta has
    // no canonical_crate. So the canonical_candidate derivation
    // should pick `crate-a` (the one crate in crates[] that appears
    // in any :Context.canonical_crate).
    assert_eq!(
        row.get("canonical_candidate").and_then(|v| v.as_str()),
        Some("crate-a"),
        "canonical_candidate must resolve to crate-a (the only canonical_crate); row:\n{row:#}"
    );
}

#[test]
fn t3_same_context_fixture_with_no_fail_exits_zero_but_preserves_payload() {
    let fixture = tempdir().expect("fixture tempdir");
    let workspace = build_same_context_fixture(fixture.path());

    let db = tempdir().expect("db tempdir");
    run_extract(db.path(), &workspace, "t3-nofail");

    let output = run_check_t3(db.path(), "t3-nofail", true);
    assert!(
        output.status.success(),
        "`--no-fail` must exit 0 even when rows fire; stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("well-formed JSON");
    let rows = json.get("rows").and_then(|r| r.as_array()).expect("rows");
    assert_eq!(
        rows.len(),
        1,
        "`--no-fail` must not suppress rows in the payload:\n{stdout}"
    );
}

#[test]
fn t3_clean_fixture_with_no_shared_names_reports_zero_rows() {
    let fixture = tempdir().expect("fixture tempdir");
    let root = fixture.path();

    // Workspace with two crates defining DIFFERENT types — no name
    // collisions → T3 reports zero rows.
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
    write(&root.join("crate-a/src/lib.rs"), "pub struct TypeA;\n");
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
    write(&root.join("crate-b/src/lib.rs"), "pub struct TypeB;\n");

    let db = tempdir().expect("db tempdir");
    run_extract(db.path(), root, "t3-clean");

    let output = run_check_t3(db.path(), "t3-clean", false);
    assert!(
        output.status.success(),
        "clean fixture must exit 0; stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("well-formed JSON");
    let rows = json.get("rows").and_then(|r| r.as_array()).expect("rows");
    assert!(
        rows.is_empty(),
        "clean fixture must report zero T3 rows; got {}:\n{stdout}",
        rows.len()
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("violations: 0 (rule: trigger T3)"),
        "stderr must carry the zero-row summary:\n{stderr}"
    );
}

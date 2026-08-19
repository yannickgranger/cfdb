#![cfg(feature = "classify")]

use std::path::{Path, PathBuf};
use std::process::Command;

use assert_cmd::prelude::*;
use tempfile::tempdir;

mod common;

fn shared_scope_db() -> PathBuf {
    common::cached_db("scope-cfdb-v01", |db| {
        common::extract(db, &common::workspace_root(), "cfdb-v01", &[]);
    })
}

fn run_scope(db: &Path, args: &[&str]) -> std::process::Output {
    let mut cmd = Command::cargo_bin("cfdb").expect("cfdb bin");
    cmd.args(["scope", "--db", db.to_str().expect("utf-8")]);
    cmd.args(args);
    cmd.output().expect("spawn cfdb scope")
}

#[test]
fn scope_help_lists_every_flag() {
    let out = Command::cargo_bin("cfdb")
        .expect("cfdb bin")
        .args(["scope", "--help"])
        .output()
        .expect("spawn cfdb scope --help");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    for flag in [
        "--db",
        "--context",
        "--workspace",
        "--format",
        "--output",
        "--keyspace",
    ] {
        assert!(
            stdout.contains(flag),
            "scope --help missing flag `{flag}`:\n{stdout}"
        );
    }
}

#[test]
fn scope_missing_context_arg_exits_with_usage_error_code_2() {
    let out = Command::cargo_bin("cfdb")
        .expect("cfdb bin")
        .args(["scope", "--db", "/doesnt-matter"])
        .output()
        .expect("spawn");
    assert_eq!(
        out.status.code(),
        Some(2),
        "clap usage error for missing --context must exit 2"
    );
}

#[test]
fn scope_rejects_unknown_context() {
    let db = shared_scope_db();
    let out = run_scope(
        &db,
        &["--keyspace", "cfdb-v01", "--context", "does-not-exist"],
    );
    assert_eq!(
        out.status.code(),
        Some(2),
        "Usage error (unknown context) must exit 2 per RFC-044 §3.6 exit-code contract"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("unknown context") && stderr.contains("known contexts"),
        "error must mention unknown + known contexts: {stderr}"
    );
}

#[test]
fn scope_filters_to_named_context() {
    let db = shared_scope_db();
    let out = run_scope(&db, &["--keyspace", "cfdb-v01", "--context", "cfdb"]);
    assert!(
        out.status.success(),
        "scope --context cfdb failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let parsed: serde_json::Value = serde_json::from_slice(&out.stdout).expect("JSON");
    assert_eq!(parsed["context"].as_str(), Some("cfdb"));
    let loc = parsed["loc_per_crate"]
        .as_object()
        .expect("loc_per_crate object");
    assert!(
        loc.contains_key("cfdb-core") && loc.contains_key("cfdb-cli"),
        "expected cfdb-core + cfdb-cli crates in loc_per_crate: {loc:?}"
    );
    for crate_name in ["cfdb-core", "cfdb-cli", "cfdb-extractor", "cfdb-petgraph"] {
        if let Some(count) = loc.get(crate_name).and_then(|v| v.as_u64()) {
            assert!(count > 0, "{crate_name} has zero items");
        }
    }
}

#[test]
fn scope_emits_section_a33_shape() {
    let db = shared_scope_db();
    let out = run_scope(&db, &["--keyspace", "cfdb-v01", "--context", "cfdb"]);
    assert!(out.status.success());
    let parsed: serde_json::Value = serde_json::from_slice(&out.stdout).expect("JSON");
    let obj = parsed.as_object().expect("top-level object");
    for key in [
        "context",
        "keyspace_sha",
        "findings_by_class",
        "canonical_candidates",
        "reachability_map",
        "loc_per_crate",
    ] {
        assert!(obj.contains_key(key), "missing top-level key `{key}`");
    }
    assert!(obj["reachability_map"].is_null());
    let classes = obj["findings_by_class"].as_object().expect("classes");
    for expected in [
        "duplicated_feature",
        "context_homonym",
        "unfinished_refactor",
        "random_scattering",
        "canonical_bypass",
        "unwired",
    ] {
        assert!(
            classes.contains_key(expected),
            "class bucket `{expected}` missing"
        );
    }
    assert_eq!(classes.len(), 6, "unexpected class bucket count");
    assert!(obj["canonical_candidates"].is_array());
}

#[test]
fn scope_deterministic_across_runs() {
    let db = shared_scope_db();
    let a = run_scope(&db, &["--keyspace", "cfdb-v01", "--context", "cfdb"]);
    let b = run_scope(&db, &["--keyspace", "cfdb-v01", "--context", "cfdb"]);
    assert!(a.status.success() && b.status.success());
    assert_eq!(
        a.stdout, b.stdout,
        "two runs with identical args must produce identical stdout (G1)"
    );
}

#[test]
fn scope_empty_buckets_carry_per_class_warning_naming_missing_input() {
    let db = shared_scope_db();
    let out = run_scope(&db, &["--keyspace", "cfdb-v01", "--context", "cfdb"]);
    assert!(out.status.success());
    let parsed: serde_json::Value = serde_json::from_slice(&out.stdout).expect("JSON");
    let warnings = parsed["warnings"]
        .as_array()
        .expect("warnings array present even when empty");
    let combined = serde_json::to_string(warnings).expect("serialize warnings");

    for class in [
        "context_homonym",
        "random_scattering",
        "canonical_bypass",
        "unwired",
    ] {
        assert!(
            combined.contains(class),
            "expected empty-bucket warning for class `{class}`. The three \
             HIR-dependent classes (random_scattering / canonical_bypass / \
             unwired) are empty because a syn-only keyspace lacks their \
             inputs. `context_homonym` is empty because every cfdb crate \
             maps to the single `cfdb` bounded context — if THAT is the one \
             that went red, check .cfdb/concepts/cfdb.toml lists every \
             crate in the workspace before assuming a real homonym (#585). \
             warnings: {combined}"
        );
    }

    assert!(
        combined.contains("reachability_map") && combined.contains("HIR")
            || combined.contains("reachability_map") && combined.contains("cfdb-hir"),
        "expected reachability_map HIR warning: {combined}"
    );
}

#[test]
fn scope_rejects_format_table_in_v01() {
    let db = shared_scope_db();
    let out = run_scope(
        &db,
        &[
            "--keyspace",
            "cfdb-v01",
            "--context",
            "cfdb",
            "--format",
            "table",
        ],
    );
    assert_eq!(
        out.status.code(),
        Some(2),
        "--format table must exit 2 (Usage error) per RFC-044 §3.6 exit-code contract"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("table") && stderr.contains("v0.2"),
        "error must explain deferral: {stderr}"
    );
}

#[test]
fn scope_writes_to_output_path_when_given() {
    let db = shared_scope_db();
    let out_dir = tempdir().expect("out tempdir");
    let out_path = out_dir.path().join("inventory.json");
    let out = run_scope(
        &db,
        &[
            "--keyspace",
            "cfdb-v01",
            "--context",
            "cfdb",
            "--output",
            out_path.to_str().expect("utf-8"),
        ],
    );
    assert!(out.status.success());
    assert!(
        out.stdout.is_empty(),
        "stdout must be empty when --output is given"
    );
    let contents = std::fs::read_to_string(&out_path).expect("read output file");
    let parsed: serde_json::Value = serde_json::from_str(&contents).expect("JSON");
    assert_eq!(parsed["context"].as_str(), Some("cfdb"));
}

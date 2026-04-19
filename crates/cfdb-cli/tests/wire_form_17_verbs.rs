//! Wire-form coverage gate (#3629 / #3728 / #3729 / RFC §6.2 +
//! council-cfdb-wiring RATIFIED §A.14 + §A.17).
//!
//! Asserts that every one of the 17 cfdb API verbs (15 RFC §6 verbs + the
//! 16th typed convenience verb `list_items_matching` from §A.14 + the 17th
//! verb `cfdb scope` from §A.17) is exposed as a clap subcommand on the
//! `cfdb` binary, with its own `--help`. This is the Phase A acceptance
//! signal for the wire form: behavior may be a stub but the surface MUST
//! be complete.
//!
//! Adding an 18th verb means adding a row to `RFC_VERBS` here; the test
//! will fail until the CLI catches up.

use std::process::Command;

use assert_cmd::prelude::*;

/// Canonical 17-verb list: RFC-029 §6 (15 verbs for cfdb v0.1) plus the
/// 16th typed convenience verb ratified in council-cfdb-wiring RATIFIED
/// §A.14 (`list_items_matching`) plus the 17th data-aggregation verb
/// ratified in §A.17 (`scope --context`).
///
/// Each tuple is `(rfc_verb_name, cli_subcommand)`. The cli subcommand
/// usually matches the verb after `_` → `-` translation; the few rename
/// pairs (`schema_version` → `version`, `query_raw` → `query`, etc.) are
/// the same renames documented in `cfdb-cli/src/main.rs` module docs.
const RFC_VERBS: &[(&str, &str)] = &[
    // INGEST
    ("extract", "extract"),
    ("enrich_docs", "enrich-docs"),
    ("enrich_metrics", "enrich-metrics"),
    ("enrich_history", "enrich-history"),
    ("enrich_concepts", "enrich-concepts"),
    // RAW
    ("query_raw", "query"),
    // TYPED
    ("find_canonical", "find-canonical"),
    ("list_callers", "list-callers"),
    ("list_violations", "violations"),
    ("list_bypasses", "list-bypasses"),
    ("list_items_matching", "list-items-matching"),
    ("scope", "scope"),
    // SNAPSHOT
    ("list_snapshots", "snapshots"),
    ("diff", "diff"),
    ("drop", "drop"),
    // SCHEMA
    ("schema_version", "version"),
    ("schema_describe", "schema-describe"),
];

#[test]
fn every_rfc_verb_has_a_clap_subcommand_with_help() {
    for (rfc, sub) in RFC_VERBS {
        let output = Command::cargo_bin("cfdb")
            .expect("cfdb binary is built for integration tests")
            .args([sub, "--help"])
            .output()
            .unwrap_or_else(|e| panic!("spawn `cfdb {sub} --help` failed: {e}"));
        assert!(
            output.status.success(),
            "verb `{rfc}` missing — `cfdb {sub} --help` exited with {:?}\nstderr: {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("Usage:") || stdout.contains("usage:"),
            "verb `{rfc}` (`cfdb {sub} --help`) did not print a Usage line:\n{stdout}"
        );
    }
}

#[test]
fn root_help_lists_all_17_rfc_verbs() {
    let output = Command::cargo_bin("cfdb")
        .expect("cfdb binary is built for integration tests")
        .arg("--help")
        .output()
        .expect("spawn `cfdb --help`");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    for (rfc, sub) in RFC_VERBS {
        assert!(
            stdout.contains(sub),
            "root `cfdb --help` is missing verb `{rfc}` (subcommand `{sub}`):\n{stdout}"
        );
    }
}

#[test]
fn unknown_subcommand_exits_with_usage_error_code_2() {
    let output = Command::cargo_bin("cfdb")
        .expect("cfdb binary is built for integration tests")
        .arg("definitely-not-a-verb")
        .output()
        .expect("spawn cfdb");
    assert_eq!(
        output.status.code(),
        Some(2),
        "clap usage errors must exit code 2 (RFC §6.2 wire-form contract)"
    );
}

#[test]
fn schema_describe_prints_json_and_exits_zero() {
    let output = Command::cargo_bin("cfdb")
        .expect("cfdb binary is built for integration tests")
        .arg("schema-describe")
        .output()
        .expect("spawn cfdb schema-describe");
    assert!(
        output.status.success(),
        "schema-describe must exit 0; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("schema-describe stdout is not JSON: {e}\n{stdout}"));
    assert!(
        parsed.get("schema_version").is_some(),
        "schema-describe JSON missing `schema_version` field"
    );
    assert!(
        parsed.get("nodes").and_then(|v| v.as_array()).is_some(),
        "schema-describe JSON missing `nodes` array"
    );
    assert!(
        parsed.get("edges").and_then(|v| v.as_array()).is_some(),
        "schema-describe JSON missing `edges` array"
    );
}

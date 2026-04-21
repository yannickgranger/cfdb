//! Wire-form coverage gate (#3629 / #3728 / #3729 / RFC §6.2 +
//! council-cfdb-wiring RATIFIED §A.14 + §A.17 + #43 council round 1
//! synthesis §43-A).
//!
//! Asserts that every one of the 20 cfdb API verbs is exposed as a clap
//! subcommand on the `cfdb` binary, with its own `--help`. Surface:
//!
//! - 15 RFC §6 verbs for cfdb v0.1
//! - 16th typed verb `list_items_matching` from §A.14
//! - 17th data-aggregation verb `cfdb scope` from §A.17
//! - 3 new enrichment verbs from #43 council round 1 synthesis §43-A:
//!   `enrich_bounded_context`, `enrich_deprecation`, `enrich_reachability`
//!
//! #43-A also RENAMES three existing enrichment verbs to match the RFC
//! addendum §A2.2 vocabulary (breaking rename — no backward-compat
//! aliases):
//!
//! - `enrich_history` → `enrich_git_history`
//! - `enrich_docs` → `enrich_rfc_docs`
//! - `enrich_concepts` stays by name (scope narrowed to `:Concept`
//!   node materialization per DDD Q4; see `council/43/ddd.md`)
//!
//! `enrich_metrics` retained as a Phase A stub (explicitly out of #43
//! scope per RFC amendment §A2.2).
//!
//! Adding a 21st verb means adding a row to `RFC_VERBS` here AND renaming
//! this file (the filename encodes the verb count as a tripwire).

use std::process::Command;

use assert_cmd::prelude::*;

/// Canonical 20-verb list (post #43-A — see module doc).
///
/// Each tuple is `(rfc_verb_name, cli_subcommand)`. The cli subcommand
/// usually matches the verb after `_` → `-` translation; the few rename
/// pairs (`schema_version` → `version`, `query_raw` → `query`, etc.) are
/// the same renames documented in `cfdb-cli/src/main.rs` module docs.
const RFC_VERBS: &[(&str, &str)] = &[
    // INGEST
    ("extract", "extract"),
    // Enrichment passes (RFC addendum §A2.2 — 6-row pass table post
    // #43-A amendment + `enrich_metrics` deferred stub).
    ("enrich_git_history", "enrich-git-history"),
    ("enrich_rfc_docs", "enrich-rfc-docs"),
    ("enrich_deprecation", "enrich-deprecation"),
    ("enrich_bounded_context", "enrich-bounded-context"),
    ("enrich_concepts", "enrich-concepts"),
    ("enrich_reachability", "enrich-reachability"),
    ("enrich_metrics", "enrich-metrics"),
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
fn root_help_lists_all_20_rfc_verbs() {
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

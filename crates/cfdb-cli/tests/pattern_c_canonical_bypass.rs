//! Pattern C (RFC-029 v0.2 §A1.4) — 4-verdict canonical-bypass scar test.
//!
//! Supersedes the specialized `ledger_canonical_bypass.rs` test shipped in
//! commit `349b153d6` (v0.1 proof-of-concept). The specialized rule only
//! knew one verdict (effectively BYPASS_*, unqualified on reachability).
//! v0.2 generalizes the detector to four verdicts against any
//! `:CANONICAL_FOR`-declared concept:
//!
//!   CANONICAL_CALLER      — caller uses the canonical wire-level method
//!   BYPASS_REACHABLE      — caller uses the bypass, reached from :EntryPoint
//!   BYPASS_DEAD           — caller uses the bypass, NOT reached from :EntryPoint
//!   CANONICAL_UNREACHABLE — canonical :Item carries CANONICAL_FOR but is unreachable
//!
//! The fixture at `examples/queries/fixtures/canonical-bypass/` is shaped
//! to produce at least one row per verdict in a single extract:
//!
//!   LedgerService::record_trade       → BYPASS_REACHABLE   (reproduces #3525)
//!   LedgerService::record_trade_safe  → CANONICAL_CALLER
//!   LedgerService::record_orphan      → BYPASS_DEAD        (reproduces #3544/#3545/#3546)
//!   LedgerService::record_isolated    → CANONICAL_UNREACHABLE (reproduces #1526)
//!
//! The scar also pins the `is_test` filter: test-only bypass calls MUST
//! NOT surface under BYPASS_REACHABLE or BYPASS_DEAD (test fixtures
//! legitimately exercise wire-level forms).
//!
//! # Why this test lives in `cfdb-cli/tests/`
//!
//! The scar test needs the full pipeline: `cfdb extract --features hir`
//! (for `:EntryPoint` + `CALLS`), `cfdb enrich-concepts` (for
//! `:CANONICAL_FOR`), `cfdb enrich-reachability` (for
//! `reachable_from_entry`), and `cfdb violations` (the Cypher parser +
//! evaluator). Only `cfdb-cli` has all three crates as deps:
//! `cfdb-query`, `cfdb-petgraph`, and the `hir` feature that pulls in
//! `cfdb-hir-extractor`. A placement in `cfdb-petgraph/tests/` would
//! force that crate to depend on `cfdb-query` and `cfdb-hir-extractor`,
//! which the architecture test at
//! `cfdb-petgraph/tests/architecture_dep_rule.rs` forbids (both are on
//! the FORBIDDEN_DEPS list).
//!
//! The HIR gating also means the test requires the `hir` feature — it
//! is annotated `#[cfg(feature = "hir")]` and skipped on default builds.
//! CI runs cfdb-cli test suite both with and without the feature; the
//! scar runs under the hir profile.

#![cfg(feature = "hir")]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use assert_cmd::prelude::*;
use tempfile::tempdir;

/// Absolute path to `examples/queries/`. The scar test reads the fixture
/// and rule files from the repo-local source tree — they are not
/// `include_str!`'d because the extractor needs a real Cargo workspace
/// on disk (VFS paths are file-system-backed).
fn queries_dir() -> PathBuf {
    let cfdb_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("cfdb-cli crate dir has a parent crates/")
        .parent()
        .expect("crates/ has a parent cfdb sub-workspace root");
    cfdb_root.join("examples/queries")
}

fn fixture_dir() -> PathBuf {
    queries_dir().join("fixtures/canonical-bypass")
}

fn rule(name: &str) -> PathBuf {
    queries_dir().join(name)
}

/// Copy the repo-local fixture tree into a tempdir so the test's
/// `cfdb extract` run and its `.cfdb/db/` output do not race with
/// parallel runs against the same source tree.
fn copy_fixture(dst: &Path) {
    let src = fixture_dir();
    copy_dir_recursive(&src, dst);
}

fn copy_dir_recursive(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).expect("mkdir fixture dst");
    for entry in fs::read_dir(src).expect("read fixture src") {
        let entry = entry.expect("fixture dir entry");
        let ft = entry.file_type().expect("fixture file type");
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if ft.is_dir() {
            copy_dir_recursive(&from, &to);
        } else if ft.is_file() {
            fs::copy(&from, &to).expect("copy fixture file");
        }
    }
}

fn cfdb() -> Command {
    Command::cargo_bin("cfdb").expect("cfdb binary is built for integration tests")
}

/// Common params for the bypass rules. Every rule accepts `$concept` for
/// output-provenance; the bypass rules also take `$bypass_callee_name` +
/// `$caller_regex`; the canonical rule only needs `$concept`.
const PARAMS_BYPASS: &str =
    r#"{"concept":"ledger","bypass_callee_name":"append","caller_regex":".*::LedgerService::.*"}"#;
const PARAMS_CANONICAL: &str = r#"{"concept":"ledger","canonical_callee_name":"append_idempotent","caller_regex":".*::LedgerService::.*"}"#;
const PARAMS_UNREACHABLE: &str = r#"{"concept":"ledger"}"#;

/// Build the fixture DB, run all three enrichment passes, and return
/// the (db_dir, keyspace) pair ready for repeated `violations` calls.
fn build_and_enrich(tmp: &Path) -> (PathBuf, &'static str) {
    let workspace = tmp.join("workspace");
    copy_fixture(&workspace);
    let db = tmp.join("db");
    let ks = "fixture";

    // 1. Extract with HIR so :EntryPoint + CALLS edges land. The `--hir`
    //    flag requires the cfdb-cli binary to be built with the `hir`
    //    Cargo feature; the surrounding `#![cfg(feature = "hir")]` on
    //    this file guarantees that's the case when the test compiles.
    cfdb()
        .args([
            "extract",
            "--workspace",
            workspace.to_str().expect("workspace path utf-8"),
            "--db",
            db.to_str().expect("db path utf-8"),
            "--keyspace",
            ks,
            "--hir",
        ])
        .assert()
        .success();

    // 2. enrich_concepts — reads .cfdb/concepts/ledger.toml, emits
    //    :Concept + LABELED_AS + CANONICAL_FOR edges.
    cfdb()
        .args([
            "enrich-concepts",
            "--db",
            db.to_str().expect("db path utf-8"),
            "--keyspace",
            ks,
            "--workspace",
            workspace.to_str().expect("workspace path utf-8"),
        ])
        .assert()
        .success();

    // 3. enrich_reachability — BFS from :EntryPoint over CALLS*,
    //    writes :Item.reachable_from_entry + .reachable_entry_count.
    cfdb()
        .args([
            "enrich-reachability",
            "--db",
            db.to_str().expect("db path utf-8"),
            "--keyspace",
            ks,
        ])
        .assert()
        .success();

    (db, ks)
}

/// Run a rule and return its captured stdout. Uses `cfdb query` (not
/// `cfdb violations`) because `violations` has no `--params` flag (it
/// takes only `--rule <path>`). The Pattern C rules are parameterized
/// on `$concept`, `$bypass_callee_name`, etc, so they MUST be executed
/// through the `query` verb which does thread `--params` into the
/// evaluator. Once `violations --params` ships as a follow-up slice,
/// this helper can collapse to the shorter form.
fn run_rule(db: &Path, ks: &str, rule_file: &Path, params: &str) -> String {
    let cypher = fs::read_to_string(rule_file).expect("read rule file");
    let output = cfdb()
        .args([
            "query",
            "--db",
            db.to_str().expect("db path utf-8"),
            "--keyspace",
            ks,
            "--params",
            params,
            &cypher,
        ])
        .output()
        .expect("run cfdb query");
    String::from_utf8(output.stdout).expect("query stdout utf-8")
}

// ---------------------------------------------------------------------------
// CANONICAL_CALLER — record_trade_safe + record_isolated surface, bypass
// callers do NOT.
// ---------------------------------------------------------------------------

#[test]
fn canonical_caller_rule_finds_only_canonical_form_invocations() {
    let tmp = tempdir().expect("tempdir");
    let (db, ks) = build_and_enrich(tmp.path());
    let stdout = run_rule(
        &db,
        ks,
        &rule("canonical-bypass-caller.cypher"),
        PARAMS_CANONICAL,
    );

    assert!(
        stdout.contains("record_trade_safe"),
        "record_trade_safe uses append_idempotent → must surface as CANONICAL_CALLER:\n{stdout}"
    );
    assert!(
        stdout.contains("record_isolated"),
        "record_isolated uses append_idempotent → must surface as CANONICAL_CALLER:\n{stdout}"
    );
    assert!(
        !stdout.contains("record_trade\""),
        "record_trade uses append (bypass) → must NOT surface as CANONICAL_CALLER:\n{stdout}"
    );
    assert!(
        !stdout.contains("record_orphan"),
        "record_orphan uses append (bypass) → must NOT surface as CANONICAL_CALLER:\n{stdout}"
    );
}

// ---------------------------------------------------------------------------
// BYPASS_REACHABLE — record_trade (#3525) surfaces, NOT record_orphan.
// ---------------------------------------------------------------------------

#[test]
fn bypass_reachable_rule_reproduces_3525() {
    let tmp = tempdir().expect("tempdir");
    let (db, ks) = build_and_enrich(tmp.path());
    let stdout = run_rule(
        &db,
        ks,
        &rule("canonical-bypass-reachable.cypher"),
        PARAMS_BYPASS,
    );

    assert!(
        stdout.contains("record_trade"),
        "record_trade is reached via cli::run_record and calls the bypass append() \
         — must surface as BYPASS_REACHABLE (reproducing qbot-core #3525):\n{stdout}"
    );
    assert!(
        !stdout.contains("record_orphan"),
        "record_orphan is NOT reached from any :EntryPoint → must NOT surface \
         as BYPASS_REACHABLE (it is BYPASS_DEAD):\n{stdout}"
    );
    assert!(
        !stdout.contains("record_trade_safe"),
        "record_trade_safe uses the canonical form → must NOT surface as BYPASS_*:\n{stdout}"
    );
    assert!(
        !stdout.contains("seed_fixture"),
        "seed_fixture is test-only → is_test filter MUST drop it:\n{stdout}"
    );
}

// ---------------------------------------------------------------------------
// BYPASS_DEAD — record_orphan (#3544/#3545/#3546 shape) surfaces,
// NOT record_trade.
// ---------------------------------------------------------------------------

#[test]
fn bypass_dead_rule_reproduces_3544_3545_3546() {
    let tmp = tempdir().expect("tempdir");
    let (db, ks) = build_and_enrich(tmp.path());
    let stdout = run_rule(
        &db,
        ks,
        &rule("canonical-bypass-dead.cypher"),
        PARAMS_BYPASS,
    );

    assert!(
        stdout.contains("record_orphan"),
        "record_orphan calls the bypass append() and is NOT reached from any \
         :EntryPoint → must surface as BYPASS_DEAD (reproducing qbot-core \
         #3544/#3545/#3546 scatter shape):\n{stdout}"
    );
    assert!(
        !stdout.contains("record_trade\""),
        "record_trade IS reached via cli::run_record → must NOT surface as \
         BYPASS_DEAD (it is BYPASS_REACHABLE):\n{stdout}"
    );
    assert!(
        !stdout.contains("seed_fixture"),
        "seed_fixture is test-only → is_test filter MUST drop it:\n{stdout}"
    );
}

// ---------------------------------------------------------------------------
// CANONICAL_UNREACHABLE — record_isolated (#1526 safety-envelope shape)
// surfaces because it carries CANONICAL_FOR and no :EntryPoint reaches it.
// ---------------------------------------------------------------------------

#[test]
fn canonical_unreachable_rule_reproduces_1526() {
    let tmp = tempdir().expect("tempdir");
    let (db, ks) = build_and_enrich(tmp.path());
    let stdout = run_rule(
        &db,
        ks,
        &rule("canonical-unreachable.cypher"),
        PARAMS_UNREACHABLE,
    );

    assert!(
        stdout.contains("record_isolated"),
        "record_isolated lives in the canonical crate (CANONICAL_FOR edge) and \
         is NOT reached from any :EntryPoint → must surface as \
         CANONICAL_UNREACHABLE (reproducing qbot-core #1526 shape):\n{stdout}"
    );
    // record_trade_safe IS reached from the CLI, so even though it also
    // lives in the canonical crate and carries CANONICAL_FOR, it must
    // NOT surface as CANONICAL_UNREACHABLE.
    assert!(
        !stdout.contains("record_trade_safe"),
        "record_trade_safe is reached via cli::run_record → must NOT surface \
         as CANONICAL_UNREACHABLE:\n{stdout}"
    );
}

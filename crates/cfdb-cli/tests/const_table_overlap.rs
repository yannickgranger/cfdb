//! `const-table-overlap.cypher` rule scar (RFC-040 slice 4/5, issue #326).
//!
//! Pins the v0.1 DUPLICATE branch against
//! `examples/queries/fixtures/const-table-overlap/`:
//!
//! - `kraken_normalize::FIAT` ⟷ `oanda_pricing::FIAT` — DUPLICATE
//!   (same set, different declaration order; `entries_hash`
//!   order-invariance MUST surface this pair as one row).
//! - `binance_exchange::STABLES` — clean (same element_type as FIAT but
//!   non-overlapping set; MUST NOT pair with anything).
//! - `metric_client::PORTS` — clean (different element_type; cross-type
//!   filter MUST exclude any sha256-collision-induced false pair).
//!
//! The SUBSET / INTERSECTION_HIGH branches of RFC-040 §3.4 require the
//! `entries_subset(a, b)` / `entries_jaccard(a, b)` UDFs which do not
//! yet exist in cfdb-query — see the rule file's header for the slice-4
//! acceptance check (R2 solid-architect N2) and the follow-up issue
//! tracking the UDF landing.
//!
//! # Why this lives in `cfdb-cli/tests/`
//!
//! Same placement rationale as `signature_divergent.rs`: the scar needs
//! the full pipeline (`cfdb-extractor` + `cfdb-query` + `cfdb-petgraph`)
//! and `cfdb-cli` is the only crate that depends on all three.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use assert_cmd::prelude::*;
use tempfile::tempdir;

fn queries_dir() -> PathBuf {
    let cfdb_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("cfdb-cli has parent crates/")
        .parent()
        .expect("crates/ has parent cfdb sub-workspace root");
    cfdb_root.join("examples/queries")
}

fn fixture_dir() -> PathBuf {
    queries_dir().join("fixtures/const-table-overlap")
}

fn rule(name: &str) -> PathBuf {
    queries_dir().join(name)
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
    Command::cargo_bin("cfdb").expect("cfdb binary built for integration tests")
}

fn extract(tmp: &Path) -> (PathBuf, &'static str) {
    let workspace = tmp.join("workspace");
    copy_dir_recursive(&fixture_dir(), &workspace);
    let db = tmp.join("db");
    let ks = "ctoverlap";

    cfdb()
        .args([
            "extract",
            "--workspace",
            workspace.to_str().expect("workspace path utf-8"),
            "--db",
            db.to_str().expect("db path utf-8"),
            "--keyspace",
            ks,
        ])
        .assert()
        .success();

    (db, ks)
}

fn run_rule(db: &Path, ks: &str, rule_file: &Path) -> String {
    let cypher = fs::read_to_string(rule_file).expect("read rule file");
    let output = cfdb()
        .args([
            "query",
            "--db",
            db.to_str().expect("db path utf-8"),
            "--keyspace",
            ks,
            &cypher,
        ])
        .output()
        .expect("run cfdb query");
    String::from_utf8(output.stdout).expect("query stdout utf-8")
}

// ---------------------------------------------------------------------------
// DUPLICATE pair: kraken_normalize::FIAT ⟷ oanda_pricing::FIAT MUST surface.
// ---------------------------------------------------------------------------

#[test]
fn duplicate_fiat_pair_surfaces_under_const_table_overlap() {
    let tmp = tempdir().expect("tempdir");
    let (db, ks) = extract(tmp.path());
    let stdout = run_rule(&db, ks, &rule("const-table-overlap.cypher"));

    assert!(
        stdout.contains("CONST_TABLE_DUPLICATE"),
        "DUPLICATE pair must surface with the verdict label:\n{stdout}"
    );
    assert!(
        stdout.contains("kraken_normalize::FIAT"),
        "row must cite kraken_normalize::FIAT as one member of the pair:\n{stdout}"
    );
    assert!(
        stdout.contains("oanda_pricing::FIAT"),
        "row must cite oanda_pricing::FIAT as the other member:\n{stdout}"
    );
}

#[test]
fn duplicate_pair_is_reported_once_via_qname_lex_dedup() {
    // The MATCH (a:ConstTable), (b:ConstTable) form yields both
    // (a, b) and (b, a) without the `a.qname < b.qname` guard. The
    // rule must dedupe the symmetric pair so each unordered pair
    // appears exactly once.
    let tmp = tempdir().expect("tempdir");
    let (db, ks) = extract(tmp.path());
    let stdout = run_rule(&db, ks, &rule("const-table-overlap.cypher"));

    let n = stdout.matches("CONST_TABLE_DUPLICATE").count();
    assert_eq!(
        n, 1,
        "expected exactly one DUPLICATE row (qname lex-dedup); got {n}\n{stdout}"
    );
}

// ---------------------------------------------------------------------------
// Non-overlapping str table MUST NOT surface.
// ---------------------------------------------------------------------------

#[test]
fn binance_stables_does_not_pair_with_unrelated_str_tables() {
    let tmp = tempdir().expect("tempdir");
    let (db, ks) = extract(tmp.path());
    let stdout = run_rule(&db, ks, &rule("const-table-overlap.cypher"));

    assert!(
        !stdout.contains("binance_exchange::STABLES"),
        "STABLES has same element_type as FIAT but non-overlapping set — \
         must NOT surface (entries_hash join, not element_type alone):\n{stdout}"
    );
}

// ---------------------------------------------------------------------------
// Cross-element-type filter: u32 table MUST NOT pair with str tables.
// ---------------------------------------------------------------------------

#[test]
fn numeric_table_does_not_pair_with_string_tables() {
    let tmp = tempdir().expect("tempdir");
    let (db, ks) = extract(tmp.path());
    let stdout = run_rule(&db, ks, &rule("const-table-overlap.cypher"));

    assert!(
        !stdout.contains("metric_client::PORTS"),
        "PORTS is element_type=u32; the rule filters \
         a.element_type = b.element_type — must NOT pair with any \
         &str table:\n{stdout}"
    );
}

// ---------------------------------------------------------------------------
// G1 determinism: rule output is byte-stable across two extracts.
// ---------------------------------------------------------------------------

#[test]
fn const_table_overlap_rule_output_is_byte_stable() {
    let tmp1 = tempdir().expect("tempdir1");
    let tmp2 = tempdir().expect("tempdir2");
    let (db1, ks1) = extract(tmp1.path());
    let (db2, ks2) = extract(tmp2.path());
    let out1 = run_rule(&db1, ks1, &rule("const-table-overlap.cypher"));
    let out2 = run_rule(&db2, ks2, &rule("const-table-overlap.cypher"));
    assert_eq!(
        out1, out2,
        "const-table-overlap.cypher rule output must be byte-identical \
         across two extracts (G1 determinism)"
    );
}

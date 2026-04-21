//! `signature_divergent` UDF + signature-emission scar (issue #47).
//!
//! Pins the RFC-029 §A1.5 v0.2-8 gate: the UDF must distinguish a
//! Context Homonym (same last qname segment across bounded contexts,
//! DIVERGENT `:Item.signature`) from a Shared Kernel (same last
//! segment, IDENTICAL signature). The fixture at
//! `examples/queries/fixtures/signature-divergent/` ships two crates:
//!
//!   - `trading_port::Position::valuation -> f64`
//!   - `trading_adapter::Position::valuation -> (f64, f64)`       ← DIVERGENT pair
//!   - `trading_port::OrderBook::place_order -> Result<(), _>`
//!   - `trading_adapter::OrderBook::place_order -> Result<(), _>` ← IDENTICAL pair
//!
//! The scar runs `cfdb extract` on the fixture (default / syn-only —
//! signature emission is a syn-extractor concern, not HIR, since the
//! `cfdb-hir-extractor` exclusion test forbids `Label::ITEM` emission
//! there) and then runs `examples/queries/signature-divergent.cypher`
//! via `cfdb query`. It asserts:
//!
//!   1. The `Position::valuation` pair MUST surface (Context Homonym)
//!   2. The `OrderBook::place_order` pair MUST NOT surface (Shared Kernel)
//!
//! # Why this lives in `cfdb-cli/tests/`
//!
//! Same placement rationale as `pattern_c_canonical_bypass.rs`: the
//! scar needs the full pipeline (`cfdb-extractor` + `cfdb-query` +
//! `cfdb-petgraph`), and `cfdb-cli` is the only crate that depends on
//! all three. Unlike Pattern C, this scar does NOT need the `hir`
//! feature — `signature` prop emission is a syn-extractor concern and
//! the UDF is pure. Running default-profile makes the scar a useful
//! determinism + byte-stability spot-check on signature rendering.
//!
//! # Determinism guard
//!
//! The scar also re-extracts the fixture twice and asserts the rule
//! output is byte-identical across the two runs — G1 in miniature
//! for the signature prop. `render_fn_signature` is deliberately
//! source-order-only (no HashMap / HashSet anywhere in its call tree),
//! but a regression that introduces nondeterminism here (e.g. via a
//! future attribute-ordering change) would flip this assertion.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use assert_cmd::prelude::*;
use tempfile::tempdir;

fn queries_dir() -> PathBuf {
    let cfdb_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("cfdb-cli crate dir has a parent crates/")
        .parent()
        .expect("crates/ has a parent cfdb sub-workspace root");
    cfdb_root.join("examples/queries")
}

fn fixture_dir() -> PathBuf {
    queries_dir().join("fixtures/signature-divergent")
}

fn rule(name: &str) -> PathBuf {
    queries_dir().join(name)
}

fn copy_fixture(dst: &Path) {
    copy_dir_recursive(&fixture_dir(), dst);
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

/// Build the fixture DB via the default (syn-only) extractor.
/// `:Item.signature` is a syn-extractor prop; no HIR needed.
fn extract(tmp: &Path) -> (PathBuf, &'static str) {
    let workspace = tmp.join("workspace");
    copy_fixture(&workspace);
    let db = tmp.join("db");
    let ks = "sigdiv";

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
// DIVERGENT pair: Position::valuation MUST surface.
// ---------------------------------------------------------------------------

#[test]
fn divergent_valuation_pair_surfaces_as_context_homonym() {
    let tmp = tempdir().expect("tempdir");
    let (db, ks) = extract(tmp.path());
    let stdout = run_rule(&db, ks, &rule("signature-divergent.cypher"));

    assert!(
        stdout.contains("valuation"),
        "Position::valuation has DIVERGENT signatures across trading_port \
         and trading_adapter (fn(&Self) -> f64 vs fn(&Self) -> (f64, f64)) \
         — must surface under signature-divergent.cypher:\n{stdout}"
    );
    assert!(
        stdout.contains("trading_port"),
        "DIVERGENT pair row must cite trading_port as one of the bounded \
         contexts:\n{stdout}"
    );
    assert!(
        stdout.contains("trading_adapter"),
        "DIVERGENT pair row must cite trading_adapter as one of the \
         bounded contexts:\n{stdout}"
    );
}

// ---------------------------------------------------------------------------
// IDENTICAL pair: OrderBook::place_order MUST NOT surface (Shared Kernel).
// ---------------------------------------------------------------------------

#[test]
fn identical_place_order_pair_is_not_surfaced_as_homonym() {
    let tmp = tempdir().expect("tempdir");
    let (db, ks) = extract(tmp.path());
    let stdout = run_rule(&db, ks, &rule("signature-divergent.cypher"));

    assert!(
        !stdout.contains("place_order"),
        "OrderBook::place_order has IDENTICAL signatures across \
         trading_port and trading_adapter (Shared Kernel) — must NOT \
         surface under signature-divergent.cypher (route to \
         /sweep-epic would incorrectly dedupe intentional shared \
         contract):\n{stdout}"
    );
}

// ---------------------------------------------------------------------------
// G1 determinism: signature emission is byte-stable across extracts.
// ---------------------------------------------------------------------------

#[test]
fn signature_emission_is_byte_stable_across_extracts() {
    let tmp = tempdir().expect("tempdir");
    let (db, ks) = extract(tmp.path());
    let out1 = run_rule(&db, ks, &rule("signature-divergent.cypher"));

    // Second extract into a fresh DB on a fresh workspace copy —
    // exercises the full render_fn_signature path twice.
    let tmp2 = tempdir().expect("tempdir2");
    let (db2, ks2) = extract(tmp2.path());
    let out2 = run_rule(&db2, ks2, &rule("signature-divergent.cypher"));

    assert_eq!(
        out1, out2,
        "signature-divergent.cypher output diverged across two extracts \
         of the same fixture — `:Item.signature` emission is not \
         byte-stable (G1 violation)."
    );
}

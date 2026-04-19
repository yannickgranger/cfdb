//! RFC-029 §13 Item 1 — Phase C harness (#3637).
//!
//! Runs `arch-ban-utc-now.cypher` against the REAL qbot-core source tree
//! via the `cfdb_cli` Rust lib form (no subprocess). Replaces the
//! handwritten `architecture_test_banning_utc_now.rs` reference test
//! deleted in the same PR.
//!
//! This harness is the v0.1 production-authority signal: when `Utc::now()`
//! leaks into a `domain-*` or `ports-*` prod path, this test fails in CI
//! without any hand-rolled AST walker.
//!
//! # Mode: inventory (non-empty assertion)
//!
//! The #3573 sweep of `Utc::now` from domain/projection code has not yet
//! landed. Until it does, this test asserts `row_count > 0` — it is an
//! inventory, not a strict ban. Issue #3638 (the architecture-rfc-enforcement
//! CI gate, closes #3578) will flip the assertion to `row_count == 0` once
//! develop is clean. Same posture as the deleted AST reference test.
//!
//! # Lib form (AC-4)
//!
//! Calls `cfdb_cli::extract` and `cfdb_cli::violations` directly. No
//! `Command::cargo_bin`, no `std::process::Command`. The existing
//! subprocess-form harness (`arch_ban_utc_now.rs` against a synthetic
//! fixture) stays as-is for its own coverage; this file is the lib-form
//! real-tree companion.
//!
//! # Why the qbot-core root is 4 levels up
//!
//! `CARGO_MANIFEST_DIR` points at `.concept-graph/cfdb/crates/cfdb-cli`.
//! Going up: `cfdb-cli` → `crates` → `cfdb` → `.concept-graph` → qbot-core
//! workspace root. That's 4 parent hops.

use std::path::{Path, PathBuf};

use tempfile::tempdir;

fn qbot_core_root() -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest
        .ancestors()
        .nth(4)
        .expect("qbot-core root is 4 parents up from cfdb-cli/CARGO_MANIFEST_DIR")
        .to_path_buf()
}

fn arch_ban_utc_now_rule(cfdb_root: &Path) -> PathBuf {
    cfdb_root.join("examples/queries/arch-ban-utc-now.cypher")
}

fn cfdb_sub_workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("cfdb-cli manifest dir has a parent crates/ directory")
        .parent()
        .expect("crates/ has a parent cfdb sub-workspace root")
        .to_path_buf()
}

// `#[ignore]` by default: extraction over the full qbot-core source tree
// takes ~150s, well above the ~30s ceiling for default `cargo test`. Run
// explicitly via `cargo test --test arch_ban_utc_now_real_tree -- --ignored`
// or opt-in from CI (issue #3638 wires the architecture-rfc-enforcement
// workflow to run this and other ignored cfdb gates).
#[test]
#[ignore = "slow (~150s extract) — opt-in via --ignored or #3638 CI gate"]
fn arch_ban_utc_now_inventory_on_real_qbot_core_tree() {
    let workspace = qbot_core_root();
    let cfdb_root = cfdb_sub_workspace_root();
    let rule = arch_ban_utc_now_rule(&cfdb_root);

    assert!(
        rule.is_file(),
        "arch-ban-utc-now.cypher must exist at {}",
        rule.display()
    );
    assert!(
        workspace.join("Cargo.toml").is_file(),
        "qbot-core workspace Cargo.toml must exist at {}",
        workspace.display()
    );

    let db = tempdir().expect("db tempdir");

    // 1. Extract the real qbot-core source via LIB FORM.
    cfdb_cli::extract(
        workspace.clone(),
        db.path().to_path_buf(),
        Some("qbot_core_real".to_string()),
    )
    .expect("cfdb_cli::extract against real qbot-core tree must succeed");

    // 2. Run the rule via LIB FORM.
    let row_count = cfdb_cli::violations(
        db.path().to_path_buf(),
        "qbot_core_real".to_string(),
        rule.clone(),
    )
    .expect("cfdb_cli::violations with arch-ban-utc-now.cypher must succeed");

    eprintln!(
        "arch_ban_utc_now_real_tree: {} Utc::now row(s) flagged by cypher rule \
         against real qbot-core tree",
        row_count
    );

    // 3. Inventory-mode assertion. Until #3573 sweeps `Utc::now` from
    //    domain/projection code, this test observes a non-zero inventory.
    //    #3638 flips this to `row_count == 0` once the sweep lands.
    assert!(
        row_count > 0,
        "cypher rule found zero Utc::now call sites on the real qbot-core tree — \
         either #3573 has swept domain/projection clean (flip this assertion to \
         `row_count == 0` as part of #3638), or the extractor/rule has regressed. \
         Check both before relaxing."
    );
}

//! Pattern C felt win (RFC §3.3) — canonical bypass detection.
//!
//! Fixture: a synthetic `ledger` crate with a `LedgerService` struct
//! that has two methods. One (`record_trade`) calls the non-canonical
//! `.append(...)` method — this is exactly the #3525 shape. The other
//! (`record_trade_safe`) calls `.append_idempotent(...)`, the
//! canonical form. The ban rule must surface the first and stay
//! silent on the second.
//!
//! Also includes a #[cfg(test)] test that calls `.append()` — the
//! `is_test` filter in the rule MUST drop it, because test harness
//! code legitimately bypasses idempotency guards to build scenarios.

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

fn build_fixture_workspace(root: &Path) -> PathBuf {
    write(
        &root.join("Cargo.toml"),
        r#"[workspace]
resolver = "2"
members = ["ledger"]
"#,
    );
    write(
        &root.join("ledger/Cargo.toml"),
        r#"[package]
name = "ledger"
version = "0.0.1"
edition = "2021"
publish = false

[lib]
path = "src/lib.rs"
"#,
    );
    write(
        &root.join("ledger/src/lib.rs"),
        r#"// Synthetic LedgerService with the #3525 shape. No real
// chrono/tokio deps — syn is text-based so we don't need the types
// to resolve.

pub struct LedgerService<R> {
    repo: R,
}

impl<R: LedgerRepository> LedgerService<R> {
    // The filed bug: non-canonical `.append()` on the write path.
    pub async fn record_trade(&self) {
        let entries = build_entries();
        let _ = self.repo.append(entries).await;
    }

    // The canonical caller — uses `.append_idempotent()`. The rule
    // MUST NOT surface this one.
    pub async fn record_trade_safe(&self) {
        let entries = build_entries();
        let _ = self.repo.append_idempotent("ref", entries).await;
    }
}

pub trait LedgerRepository {
    async fn append(&self, entries: Vec<()>);
    async fn append_idempotent(&self, external_ref: &str, entries: Vec<()>);
}

fn build_entries() -> Vec<()> { Vec::new() }

#[cfg(test)]
mod tests {
    use super::*;

    pub struct MockRepo;

    // A test harness impl that legitimately uses `.append()` to build
    // fixtures. The `is_test` filter in the rule MUST drop it.
    impl LedgerService<MockRepo> {
        pub async fn seed_fixture(&self) {
            let _ = self.repo.append(Vec::new()).await;
        }
    }
}
"#,
    );

    root.to_path_buf()
}

fn rule_path() -> PathBuf {
    let cfdb_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("cfdb-cli manifest dir has a parent crates/ directory")
        .parent()
        .expect("crates/ has a parent cfdb sub-workspace root");
    cfdb_root.join("examples/queries/ledger-canonical-bypass.cypher")
}

#[test]
fn ledger_canonical_bypass_catches_record_trade_only() {
    let fixture = tempdir().expect("fixture tempdir");
    let workspace = build_fixture_workspace(fixture.path());
    let db = tempdir().expect("db tempdir");

    // 1. Extract.
    Command::cargo_bin("cfdb")
        .expect("cfdb binary is built for integration tests")
        .args([
            "extract",
            "--workspace",
            workspace
                .to_str()
                .expect("fixture workspace tempdir path is valid utf-8"),
            "--db",
            db.path().to_str().expect("db tempdir path is valid utf-8"),
            "--keyspace",
            "fixture",
        ])
        .assert()
        .success();

    // 2. Run the rule via the violations subcommand with --no-fail so
    //    we can inspect the output regardless of exit code.
    let output = Command::cargo_bin("cfdb")
        .expect("cfdb binary is built for integration tests")
        .args([
            "violations",
            "--db",
            db.path().to_str().expect("db tempdir path is valid utf-8"),
            "--keyspace",
            "fixture",
            "--rule",
            rule_path()
                .to_str()
                .expect("cfdb rule file path is valid utf-8"),
            "--no-fail",
        ])
        .output()
        .expect("run cfdb violations");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // 3. The bypass caller MUST appear.
    assert!(
        stdout.contains("record_trade"),
        "expected record_trade in violations, got:\n{stdout}"
    );

    // 4. The canonical caller MUST NOT appear — it uses append_idempotent.
    assert!(
        !stdout.contains("record_trade_safe"),
        "record_trade_safe uses the canonical and must not surface:\n{stdout}"
    );

    // 5. The test harness fn MUST NOT appear — `is_test` filter drops it.
    assert!(
        !stdout.contains("seed_fixture"),
        "seed_fixture is test-only and must be filtered out:\n{stdout}"
    );

    // 6. Exactly one violation row (just record_trade).
    assert!(
        stderr.contains("violations: 1"),
        "expected `violations: 1` on stderr, got:\n{stderr}"
    );

    // 7. Without --no-fail, the command MUST exit non-zero.
    let fail_status = Command::cargo_bin("cfdb")
        .expect("cfdb binary is built for integration tests")
        .args([
            "violations",
            "--db",
            db.path().to_str().expect("db tempdir path is valid utf-8"),
            "--keyspace",
            "fixture",
            "--rule",
            rule_path()
                .to_str()
                .expect("cfdb rule file path is valid utf-8"),
        ])
        .status()
        .expect("run cfdb violations (fail mode)");
    assert!(
        !fail_status.success(),
        "violations subcommand without --no-fail must exit non-zero on hits"
    );
}

//! RFC §13 headline acceptance test (Q1=(b), Pattern D).
//!
//! Proves cfdb replaces a handwritten Rust architecture test with one
//! `.cypher` file. Builds a synthetic workspace containing one crate that
//! violates the rule (`chrono::Utc::now()` inside a domain-prefixed crate)
//! and one that doesn't, runs the extractor via the `cfdb` binary, then
//! evaluates `examples/queries/arch-ban-utc-now.cypher` against the result
//! and asserts the violation is surfaced while the clean crate is silent.
//!
//! This is the "felt win" — on a clean tree the query returns no rows; on
//! a tree with a regression it returns the exact file + qname + callee path.

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

/// Build a synthetic Cargo workspace with two member crates:
/// - `domain-trading` calls `chrono::Utc::now()` in a fn body → violation
/// - `domain-clean`   has no Utc::now call                     → silent
fn build_fixture_workspace(root: &Path) -> PathBuf {
    write(
        &root.join("Cargo.toml"),
        r#"[workspace]
resolver = "2"
members = ["domain-trading", "domain-clean"]
"#,
    );

    // Violator crate.
    write(
        &root.join("domain-trading/Cargo.toml"),
        r#"[package]
name = "domain-trading"
version = "0.0.1"
edition = "2021"
publish = false

[lib]
path = "src/lib.rs"
"#,
    );
    write(
        &root.join("domain-trading/src/lib.rs"),
        r#"//! Synthetic violator. The extractor parses this via `syn` — the
//! `chrono` crate does not need to exist in the fixture's Cargo.lock for
//! the name-based CallSite extractor to observe the path.

// Aliased import per RFC-029 §13 Item 1 AC5: `use chrono::Utc as NotUtc;`.
// `NotUtc::now` ends in `Utc::now`, so the cypher's `.*Utc::now` regex
// catches it as an incidental true positive (QA-3 — extra true positives
// are improvements, not failures).
use chrono::Utc as NotUtc;

pub fn stamp_trade() {
    let _now = chrono::Utc::now();
}

pub fn stamp_aliased() {
    let _now = NotUtc::now();
}

pub struct Clock;

impl Clock {
    pub fn current() -> i64 {
        let t = chrono::Utc::now();
        t.timestamp()
    }
}

// The test module MUST be ignored by the ban rule — tests legitimately
// need a clock to build fixtures. The extractor tags items inside
// `#[cfg(test)]` with `is_test=true` and the .cypher filters them out.
// `cheat_with_utc_now` here should NOT surface in the query output.
#[cfg(test)]
mod tests {
    #[test]
    fn cheat_with_utc_now() {
        let _cheat = chrono::Utc::now();
        assert!(true);
    }
}
"#,
    );

    // Clean crate.
    write(
        &root.join("domain-clean/Cargo.toml"),
        r#"[package]
name = "domain-clean"
version = "0.0.1"
edition = "2021"
publish = false

[lib]
path = "src/lib.rs"
"#,
    );
    write(
        &root.join("domain-clean/src/lib.rs"),
        r#"pub fn add(a: i64, b: i64) -> i64 {
    a + b
}
"#,
    );

    root.to_path_buf()
}

fn arch_ban_utc_now_cypher() -> String {
    // Resolved from this crate's manifest: cfdb-cli → cfdb/ → examples/queries.
    let cfdb_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("cfdb-cli manifest dir has a parent crates/ directory")
        .parent()
        .expect("crates/ has a parent cfdb sub-workspace root");
    let path = cfdb_root.join("examples/queries/arch-ban-utc-now.cypher");
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

#[test]
fn arch_ban_utc_now_finds_violator_in_fixture_workspace() {
    let fixture = tempdir().expect("fixture tempdir");
    let workspace = build_fixture_workspace(fixture.path());

    let db = tempdir().expect("db tempdir");

    // 1. Extract the fixture workspace into a keyspace.
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

    // 2. Run arch-ban-utc-now.cypher.
    let cypher = arch_ban_utc_now_cypher();
    let output = Command::cargo_bin("cfdb")
        .expect("cfdb binary is built for integration tests")
        .args([
            "query",
            "--db",
            db.path().to_str().expect("db tempdir path is valid utf-8"),
            "--keyspace",
            "fixture",
            &cypher,
        ])
        .output()
        .expect("run cfdb query");

    assert!(
        output.status.success(),
        "cfdb query failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);

    // 3. The violator crate MUST show up. Three call sites in `domain-trading`:
    //    - free fn `stamp_trade`   (fully-qualified `chrono::Utc::now`)
    //    - free fn `stamp_aliased` (aliased   `NotUtc::now` per AC5)
    //    - method  `Clock::current`(fully-qualified `chrono::Utc::now`)
    assert!(
        stdout.contains("domain-trading"),
        "expected domain-trading in violations, got:\n{stdout}"
    );
    assert!(
        stdout.contains("Utc::now"),
        "expected Utc::now callee in violations, got:\n{stdout}"
    );

    // 4. The clean crate MUST NOT show up — there is no Utc::now in it.
    assert!(
        !stdout.contains("domain-clean"),
        "domain-clean should not be in violations, got:\n{stdout}"
    );

    // 5. All THREE prod Utc::now call sites (free fn + aliased free fn +
    //    impl method) must be present. The `#[cfg(test)] mod tests {
    //    cheat_with_utc_now }` violation in the same crate MUST NOT be in
    //    the output — the `is_test=false` filter in arch-ban-utc-now.cypher
    //    should drop it.
    //
    //    If this assertion becomes hit_count >= 4, the test-mod filter has
    //    regressed and is leaking #[cfg(test)] items into the prod-only
    //    rule. That would silently mask real prod violations under a flood
    //    of test false positives on the real qbot-core tree.
    let hit_count = stdout.matches("Utc::now").count();
    assert_eq!(
        hit_count, 3,
        "expected exactly 3 prod Utc::now hits (fully-qualified free fn + aliased free fn + method); any other count means the cfg(test) filter is broken OR the aliased-import AC5 regressed:\n{stdout}"
    );
    assert!(
        !stdout.contains("cheat_with_utc_now"),
        "test-mod fn leaked into prod-only arch-ban results:\n{stdout}"
    );

    // 5b. AC5 — aliased-import coverage. `use chrono::Utc as NotUtc;
    //     NotUtc::now()` must appear in the violation set as its own row.
    //     The cypher's `.*Utc::now` regex matches because `NotUtc::now`
    //     ends in `Utc::now` textually. This is the incidental-true-positive
    //     superset property per RFC-029 §13 Item 1 QA-3.
    assert!(
        stdout.contains("stamp_aliased"),
        "aliased-import violation `stamp_aliased` missing — AC5 regressed:\n{stdout}"
    );
    assert!(
        stdout.contains("NotUtc::now"),
        "aliased callee_path `NotUtc::now` missing — extractor may have resolved the alias back to the original symbol, which defeats the incidental-match design:\n{stdout}"
    );

    // 6. Same rule via the typed `cfdb violations --rule <file>`
    //    subcommand. This is the shape an architecture test actually
    //    uses: pass a rule file, get a non-zero exit code iff the rule
    //    fires. The fixture contains prod violations, so exit MUST be 1.
    let cfdb_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("cfdb-cli manifest dir has a parent crates/ directory")
        .parent()
        .expect("crates/ has a parent cfdb sub-workspace root");
    let rule_path = cfdb_root.join("examples/queries/arch-ban-utc-now.cypher");

    let violations_output = Command::cargo_bin("cfdb")
        .expect("cfdb binary is built for integration tests")
        .args([
            "violations",
            "--db",
            db.path().to_str().expect("db tempdir path is valid utf-8"),
            "--keyspace",
            "fixture",
            "--rule",
            rule_path
                .to_str()
                .expect("cfdb rule file path is valid utf-8"),
        ])
        .output()
        .expect("run cfdb violations");

    assert!(
        !violations_output.status.success(),
        "violations subcommand should exit non-zero when the rule fires"
    );
    let v_stderr = String::from_utf8_lossy(&violations_output.stderr);
    assert!(
        v_stderr.contains("violations: 3"),
        "expected `violations: 3` summary on stderr (fully-qualified free fn + aliased free fn + method), got:\n{v_stderr}"
    );

    // 7. With --no-fail, the same command exits 0 even with hits.
    //    This is the "inventory current state" mode for sweeps.
    Command::cargo_bin("cfdb")
        .expect("cfdb binary is built for integration tests")
        .args([
            "violations",
            "--db",
            db.path().to_str().expect("db tempdir path is valid utf-8"),
            "--keyspace",
            "fixture",
            "--rule",
            rule_path
                .to_str()
                .expect("cfdb rule file path is valid utf-8"),
            "--no-fail",
        ])
        .assert()
        .success();

    // 8. --count-only emits just the integer row count on stdout.
    //    Intended for capture by `ci/cross-dogfood.sh` (RFC-033 §3.2) via
    //    `rows=$(cfdb violations ... --count-only --no-fail)`. Combine with
    //    --no-fail so `set -euo pipefail` doesn't kill the script at the
    //    first non-clean rule. Stderr still carries the `violations: N`
    //    summary + any shape-lint output for diagnostic parity.
    let count_only_output = Command::cargo_bin("cfdb")
        .expect("cfdb binary is built for integration tests")
        .args([
            "violations",
            "--db",
            db.path().to_str().expect("db tempdir path is valid utf-8"),
            "--keyspace",
            "fixture",
            "--rule",
            rule_path
                .to_str()
                .expect("cfdb rule file path is valid utf-8"),
            "--count-only",
            "--no-fail",
        ])
        .output()
        .expect("run cfdb violations --count-only --no-fail");

    assert!(
        count_only_output.status.success(),
        "--count-only --no-fail must exit 0 even when the rule fires"
    );

    let stdout = String::from_utf8_lossy(&count_only_output.stdout);
    let stdout_trimmed = stdout.trim_end_matches('\n');
    assert_eq!(
        stdout_trimmed, "3",
        "expected `--count-only` stdout to be exactly the integer row count (3), got:\n{stdout:?}"
    );
    assert!(
        !stdout.contains('{'),
        "--count-only must suppress the pretty-JSON payload, got:\n{stdout}"
    );

    let stderr = String::from_utf8_lossy(&count_only_output.stderr);
    assert!(
        stderr.contains("violations: 3"),
        "stderr summary must still fire under --count-only (diagnostic parity), got:\n{stderr}"
    );
}

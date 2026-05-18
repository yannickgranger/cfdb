//! RFC-043 issue #418 test (d) — `cfdb extract --no-proc-macro` CLI
//! flag round-trip. Asserts:
//! 1. The flag is recognized by clap (no "unexpected argument" error).
//! 2. The flag appears in `cfdb extract --help` so operators can
//!    discover it.
//!
//! Validating the flag's BEHAVIOR (forces ProcMacroServerChoice::None
//! through the extract pipeline) is covered by the unit tests in
//! `cfdb-hir-extractor/tests/proc_macro_config.rs` (test (c)). This
//! file is the CLI surface contract test.

use std::process::Command;

use assert_cmd::cargo::CommandCargoExt;

/// `cfdb extract --help` mentions `--no-proc-macro` in its output.
/// Discoverability gate: an undocumented escape hatch is not a usable
/// one.
#[test]
fn extract_help_mentions_no_proc_macro_flag() {
    let out = Command::cargo_bin("cfdb")
        .expect("cfdb binary built")
        .args(["extract", "--help"])
        .output()
        .expect("cfdb extract --help runs");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let combined = format!("{stdout}\n{stderr}");
    assert!(
        combined.contains("--no-proc-macro"),
        "cfdb extract --help must advertise --no-proc-macro; got stdout:\n{stdout}\nstderr:\n{stderr}"
    );
}

/// Passing `--no-proc-macro` does NOT produce an "unexpected argument"
/// clap error. We test by passing the flag with otherwise-invalid
/// positionals — clap parses flags before the subsequent validation
/// step, so the absence of an `unexpected argument` error proves the
/// flag is recognized.
#[test]
fn extract_accepts_no_proc_macro_flag() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let out = Command::cargo_bin("cfdb")
        .expect("cfdb binary built")
        .args([
            "extract",
            "--workspace",
            tmp.path().to_str().unwrap(),
            "--db",
            tmp.path().join("db").to_str().unwrap(),
            "--no-proc-macro",
        ])
        .output()
        .expect("cfdb extract runs");
    // Extract will fail (no Cargo.toml in tmpdir) but the failure mode
    // must NOT be "unexpected argument --no-proc-macro". Any other
    // failure (e.g. workspace-not-found) means clap accepted the flag.
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("unexpected argument") || !stderr.contains("--no-proc-macro"),
        "clap rejected --no-proc-macro as unexpected; stderr:\n{stderr}"
    );
}

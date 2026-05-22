//! Integration tests for the `--require-fresh` global flag (cfdb #334).
//!
//! Per AC-5: 1 test per subcommand verifies the flag rejects an envelope when
//! `--from-ref == --to-ref`. The validation runs in `main()` BEFORE subcommand
//! dispatch, so per-subcommand coverage protects against regressions where a
//! subcommand-local `Cli::parse` shape would bypass the global check.
//!
//! All 5 tests pass non-existent file paths for the subcommand-required args.
//! Files are never opened because the freshness check exits the process before
//! `run_command()` runs.

use std::process::Command;

use assert_cmd::prelude::*;

const STALE_MARKER: &str = "from_ref equals to_ref";

fn run_with_equal_refs(subcommand_args: &[&str]) -> std::process::Output {
    let mut cmd = Command::cargo_bin("check-prelude-triggers").expect("binary built by cargo test");
    cmd.args(["--require-fresh", "--from-ref=abc123", "--to-ref=abc123"])
        .args(subcommand_args);
    cmd.output().expect("spawn check-prelude-triggers")
}

fn assert_rejected(out: &std::process::Output) {
    assert_eq!(
        out.status.code(),
        Some(1),
        "expected exit 1; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        out.stdout.is_empty(),
        "stdout must be empty when freshness check rejects (no envelope emitted); got: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains(STALE_MARKER),
        "stderr should mention the stale-refs reason; got: {stderr}"
    );
}

#[test]
fn require_fresh_rejects_c1_cross_context_when_refs_equal() {
    let out = run_with_equal_refs(&[
        "c1-cross-context",
        "--context-map=does-not-exist.toml",
        "--changed-paths=does-not-exist.txt",
    ]);
    assert_rejected(&out);
}

#[test]
fn require_fresh_rejects_c3_port_signature_when_refs_equal() {
    let out = run_with_equal_refs(&["c3-port-signature", "--changed-paths=does-not-exist.txt"]);
    assert_rejected(&out);
}

#[test]
fn require_fresh_rejects_c7_financial_precision_when_refs_equal() {
    let out = run_with_equal_refs(&[
        "c7-financial-precision",
        "--financial-precision-crates=does-not-exist.toml",
        "--changed-paths=does-not-exist.txt",
    ]);
    assert_rejected(&out);
}

#[test]
fn require_fresh_rejects_c8_pipeline_stage_when_refs_equal() {
    let out = run_with_equal_refs(&[
        "c8-pipeline-stage",
        "--pipeline-stages=does-not-exist.toml",
        "--changed-paths=does-not-exist.txt",
    ]);
    assert_rejected(&out);
}

#[test]
fn require_fresh_rejects_c9_workspace_cardinality_when_refs_equal() {
    let out = run_with_equal_refs(&[
        "c9-workspace-cardinality",
        "--workspace-root=does-not-exist",
        "--changed-paths=does-not-exist.txt",
    ]);
    assert_rejected(&out);
}

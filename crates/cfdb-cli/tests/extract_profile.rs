//! RFC-048 §3.1 / slice 48-A (#492) — `cfdb extract --profile` CLI surface.
//!
//! Covers the two properties the profile MUST hold, end-to-end through the
//! real binary against a real cargo workspace (the `spikes/qa5-utc-now`
//! fixture, the same one `ci/determinism-check.sh` uses):
//!
//! 1. **Determinism (RFC-048 §4).** `--profile` reports timings to stderr
//!    and never into the keyspace, so a profiled extract's canonical dump is
//!    byte-identical to a plain extract's. This is the load-bearing
//!    invariant — a profile that perturbs the graph is a determinism break.
//! 2. **Surface.** The flag is discoverable in `--help` and, when set, the
//!    per-phase breakdown lands on stderr over the real phase vocabulary.
//!
//! The pure-function arithmetic of the breakdown (phase timers sum to the
//! measured total within tolerance — the RFC-048 §7 Unit row) is asserted by
//! the inline unit tests in `crates/cfdb-cli/src/profile.rs`; this file is
//! the observable-behavior contract.

use std::path::{Path, PathBuf};
use std::process::Command;

use assert_cmd::prelude::*;
use tempfile::tempdir;

/// The `spikes/qa5-utc-now` fixture — a small, self-contained cargo
/// workspace (its `Cargo.toml` carries an empty `[workspace]` table). Fast
/// to extract and already a determinism-check fixture.
fn fixture_workspace() -> PathBuf {
    // CARGO_MANIFEST_DIR is <repo>/crates/cfdb-cli/; the fixture is under
    // <repo>/spikes/qa5-utc-now.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("cfdb-cli manifest dir has a parent crates/ directory")
        .parent()
        .expect("crates/ has a parent cfdb workspace root")
        .join("spikes")
        .join("qa5-utc-now")
}

/// Run `cfdb extract` against the fixture, optionally with `--profile`,
/// returning the captured (stdout, stderr). Asserts the process succeeded.
fn extract_fixture(db: &Path, keyspace: &str, profile: bool) -> (String, String) {
    let workspace = fixture_workspace();
    let mut args = vec![
        "extract".to_string(),
        "--workspace".to_string(),
        workspace.to_str().expect("fixture path utf-8").to_string(),
        "--db".to_string(),
        db.to_str().expect("db path utf-8").to_string(),
        "--keyspace".to_string(),
        keyspace.to_string(),
    ];
    if profile {
        args.push("--profile".to_string());
    }
    let out = Command::cargo_bin("cfdb")
        .expect("cfdb binary built")
        .args(&args)
        .output()
        .expect("spawn cfdb extract");
    assert!(
        out.status.success(),
        "cfdb extract (profile={profile}) failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// Canonical dump of a keyspace — the byte string the determinism gate
/// hashes.
fn dump(db: &Path, keyspace: &str) -> String {
    let out = Command::cargo_bin("cfdb")
        .expect("cfdb binary built")
        .args([
            "dump",
            "--db",
            db.to_str().expect("db path utf-8"),
            "--keyspace",
            keyspace,
        ])
        .output()
        .expect("spawn cfdb dump");
    assert!(
        out.status.success(),
        "cfdb dump failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// `--profile` is advertised in `cfdb extract --help`. An undocumented
/// diagnostic flag is not a usable one.
#[test]
fn extract_help_mentions_profile_flag() {
    let out = Command::cargo_bin("cfdb")
        .expect("cfdb binary built")
        .args(["extract", "--help"])
        .output()
        .expect("cfdb extract --help runs");
    let combined = format!(
        "{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        combined.contains("--profile"),
        "cfdb extract --help must advertise --profile; got:\n{combined}"
    );
}

/// RFC-048 §4 — the load-bearing invariant. A profiled extract and a plain
/// extract of the same workspace produce byte-identical canonical dumps: the
/// timings live on stderr and never enter the graph.
#[test]
fn profile_flag_does_not_perturb_keyspace() {
    let db_plain = tempdir().expect("tempdir");
    let db_prof = tempdir().expect("tempdir");

    extract_fixture(db_plain.path(), "prof-fixture", false);
    extract_fixture(db_prof.path(), "prof-fixture", true);

    let plain = dump(db_plain.path(), "prof-fixture");
    let profiled = dump(db_prof.path(), "prof-fixture");

    assert_eq!(
        plain, profiled,
        "--profile perturbed the canonical dump — timings must not reach the keyspace (RFC-048 §4)"
    );
}

/// `--profile` emits the per-phase breakdown to stderr over the real phase
/// vocabulary. Without `--hir`, the hir-load phase is marked skipped.
#[test]
fn profile_emits_phase_breakdown_to_stderr() {
    let db = tempdir().expect("tempdir");
    let (_stdout, stderr) = extract_fixture(db.path(), "prof-fixture", true);

    assert!(
        stderr.contains("extract phase profile"),
        "profile header missing from stderr:\n{stderr}"
    );
    for label in [
        "cargo-metadata",
        "syn-walk",
        "deferred-resolve",
        "ingest",
        "save",
        "unaccounted",
    ] {
        assert!(
            stderr.contains(label),
            "phase `{label}` missing from the breakdown:\n{stderr}"
        );
    }
    assert!(
        stderr.contains("skipped; --hir not set"),
        "a no-hir profile must mark hir-load skipped:\n{stderr}"
    );
}

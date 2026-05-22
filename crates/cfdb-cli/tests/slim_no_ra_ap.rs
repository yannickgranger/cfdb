//! Slim-build anti-bloat guard — RFC-044 §3.3 sub-band 3 (slice 044-C / #422).
//!
//! **Invariant asserted:** `cfdb-cli`'s dependency tree carries zero `ra-ap-*`
//! (rust-analyzer HIR) crates unless the opt-in `hir` feature is enabled.
//! `crates/cfdb-cli/Cargo.toml:65-69` documents this as prose ("Default builds
//! MUST have zero ra-ap-* in their dep tree", RFC-032 §3 221-227); this test
//! makes it mechanical. The `ra-ap-*` cold-compile cost (90-150s) must stay
//! out of every default / slim CLI build.
//!
//! **Mechanism:** shell out to `cargo tree` (resolution only, no compile) for
//! both the default feature set and `--no-default-features`, and assert no
//! `ra-ap` token appears in either tree. Run via the `CARGO` env var cargo
//! sets for integration tests, against this crate's own manifest.

use std::path::PathBuf;
use std::process::Command;

/// The forbidden dependency-name fragment. `cargo tree` prints crate names in
/// their hyphenated Cargo form (`ra-ap-hir`, `ra-ap-syntax`, …).
const FORBIDDEN_FRAGMENT: &str = "ra-ap";

fn cfdb_cli_manifest() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")
}

/// Run `cargo tree` for cfdb-cli with `extra_args` and return its stdout.
/// Panics (fails the test) if cargo can't be invoked or returns non-zero —
/// never silently passes on a missing/failed command.
fn cargo_tree(extra_args: &[&str]) -> String {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let manifest = cfdb_cli_manifest();
    let mut args = vec![
        "tree",
        "--manifest-path",
        manifest.to_str().expect("manifest path is valid UTF-8"),
        "--edges",
        "normal",
    ];
    args.extend_from_slice(extra_args);

    let output = Command::new(&cargo)
        .args(&args)
        .output()
        .unwrap_or_else(|e| panic!("failed to invoke `{cargo} tree {extra_args:?}`: {e}"));

    assert!(
        output.status.success(),
        "`cargo tree {extra_args:?}` exited {:?}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr),
    );

    let stdout = String::from_utf8(output.stdout).expect("cargo tree stdout is UTF-8");
    // Non-vacuity: a tree that doesn't even mention cfdb-cli means the command
    // resolved the wrong package and the ra-ap assertion would be vacuous.
    assert!(
        stdout.contains("cfdb-cli"),
        "non-vacuity guard: `cargo tree {extra_args:?}` output does not mention \
         cfdb-cli — wrong manifest/package resolved:\n{stdout}"
    );
    stdout
}

fn assert_no_ra_ap(tree: &str, profile: &str) {
    let offenders: Vec<&str> = tree
        .lines()
        .filter(|l| l.contains(FORBIDDEN_FRAGMENT))
        .collect();
    assert!(
        offenders.is_empty(),
        "cfdb-cli {profile} dep tree contains `{FORBIDDEN_FRAGMENT}-*` crates \
         (RFC-032 §3 / RFC-044 §3.3): {offenders:?}\n\
         rust-analyzer HIR crates must stay behind the opt-in `hir` feature."
    );
}

#[test]
fn slim_cfdb_cli_has_no_ra_ap_in_dep_tree() {
    assert_no_ra_ap(
        &cargo_tree(&["--no-default-features"]),
        "--no-default-features",
    );
}

#[test]
fn default_cfdb_cli_has_no_ra_ap_in_dep_tree() {
    // The `hir` feature is opt-in, so even the default (`lang-rust`) build
    // must be ra-ap-free; only `--features hir` may pull them in.
    assert_no_ra_ap(&cargo_tree(&[]), "default-features");
}

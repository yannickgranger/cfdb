use std::path::PathBuf;
use std::process::Command;

const FORBIDDEN_FRAGMENT: &str = "ra-ap";

fn cfdb_cli_manifest() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")
}

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
    assert_no_ra_ap(&cargo_tree(&[]), "default-features");
}

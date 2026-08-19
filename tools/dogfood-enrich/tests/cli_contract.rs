use assert_cmd::Command;

#[test]
fn unknown_pass_exits_runtime_error_with_enumeration() {
    let mut cmd = Command::cargo_bin("dogfood-enrich").expect("bin built");
    cmd.args([
        "--pass",
        "enrich-bogus",
        "--db",
        "/tmp/dogfood-enrich-cli-contract-db",
        "--keyspace",
        "fixture",
    ]);
    let output = cmd.output().expect("subprocess runs");
    assert_eq!(
        output.status.code(),
        Some(1),
        "unknown --pass must exit 1 (runtime error). stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unknown pass"),
        "stderr must name the failure mode. got: {stderr}"
    );
    assert!(
        stderr.contains("Valid:"),
        "stderr must enumerate valid passes. got: {stderr}"
    );
    for expected in [
        "enrich-deprecation",
        "enrich-rfc-docs",
        "enrich-bounded-context",
        "enrich-concepts",
        "enrich-reachability",
        "enrich-metrics",
        "enrich-git-history",
    ] {
        assert!(
            stderr.contains(expected),
            "valid-pass enumeration missing {expected}. got: {stderr}"
        );
    }
}

#[test]
fn missing_cfdb_binary_exits_runtime_error() {
    let mut cmd = Command::cargo_bin("dogfood-enrich").expect("bin built");
    cmd.args([
        "--pass",
        "enrich-deprecation",
        "--db",
        "/tmp/dogfood-enrich-cli-contract-db",
        "--keyspace",
        "fixture",
        "--cfdb-bin",
        "/nonexistent/cfdb-binary-zzz",
    ]);
    let output = cmd.output().expect("subprocess runs");
    assert_eq!(
        output.status.code(),
        Some(1),
        "missing cfdb-bin must exit 1 (runtime error). stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn help_includes_rfc_reference() {
    let mut cmd = Command::cargo_bin("dogfood-enrich").expect("bin built");
    cmd.arg("--help");
    let output = cmd.output().expect("subprocess runs");
    assert_eq!(output.status.code(), Some(0), "--help must exit 0");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("self-enrich") || stdout.contains("RFC-039"),
        "--help should mention self-enrich or RFC-039. got: {stdout}"
    );
}

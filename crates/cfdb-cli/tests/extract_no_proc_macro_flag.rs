use std::process::Command;

use assert_cmd::cargo::CommandCargoExt;

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
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("unexpected argument") || !stderr.contains("--no-proc-macro"),
        "clap rejected --no-proc-macro as unexpected; stderr:\n{stderr}"
    );
}

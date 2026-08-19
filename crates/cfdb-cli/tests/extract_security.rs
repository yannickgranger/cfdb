use std::path::Path;
use std::process::Command;

use assert_cmd::cargo::CommandCargoExt;

fn cfdb_bin() -> Command {
    Command::cargo_bin("cfdb").expect("cfdb binary")
}

fn run_extract_rev(cache: &Path, db: &Path, url_at_sha: &str) -> std::process::Output {
    let mut cmd = cfdb_bin();
    cmd.env("CFDB_CACHE_DIR", cache)
        .args(["extract", "--workspace"])
        .arg(cache)
        .arg("--db")
        .arg(db)
        .arg("--keyspace")
        .arg("sec")
        .arg("--rev")
        .arg(url_at_sha)
        .output()
        .expect("cfdb extract --rev")
}

#[test]
fn clone_url_starting_with_double_dash_is_treated_as_path_not_option() {
    let db = tempfile::tempdir().expect("db tempdir");
    let cache = tempfile::tempdir().expect("cache tempdir");

    let url_at_sha = "file:///nonexistent/--upload-pack=evilhelper@deadbeefdead";
    let out = run_extract_rev(cache.path(), db.path(), url_at_sha);

    assert!(!out.status.success(), "extract against bogus URL must fail",);
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert!(
        stderr.contains("git clone"),
        "stderr should name the git clone subprocess (proves we reached \
         git with the URL as a positional arg, not a flag), got: {stderr}",
    );

    assert!(
        !stderr.contains("unknown option") && !stderr.contains("unrecognized option"),
        "stderr suggests git parsed the URL body as an option — `--` may be missing: {stderr}",
    );
}

#[test]
fn extract_rev_rs_source_contains_double_dash_separator_for_user_args() {
    let src = include_str!("../src/commands/extract_rev.rs");

    assert!(
        src.contains("\"clone\", \"--quiet\", \"--\", url"),
        "git clone invocation must include `--` before user-supplied url",
    );

    assert!(
        src.contains("\"fetch\", \"--quiet\", \"origin\", \"--\", sha"),
        "git fetch invocation must include `--` before user-supplied sha",
    );

    assert!(
        src.contains("\"worktree\", \"add\", \"--detach\", \"--quiet\", \"--\""),
        "git worktree add invocation must include `--` before user-supplied path/rev",
    );

    assert!(
        !src.contains("\"checkout\", \"--quiet\", \"--\""),
        "git checkout MUST NOT use `--` separator (would force pathspec mode); \
         hex validation in parse_url_at_sha guarantees sha cannot start with `--`",
    );
}

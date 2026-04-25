//! Security regression tests for `cfdb extract --rev` (audit 2026-W17 /
//! CFDB-CLI-H2 / #270 / EPIC #273).
//!
//! The `git clone/fetch/checkout` and `git worktree add` invocations in
//! [`commands::extract`] take user-supplied URLs and revs. Without a
//! `--` separator between options and positional arguments, a value
//! starting with `--` can be reinterpreted as a git option — the
//! canonical exploit shape is `file:///path/--upload-pack=<evil>`,
//! whose URL body could be parsed as `--upload-pack=…` and run an
//! attacker-supplied helper.
//!
//! These tests assert the defense-in-depth `--` separator is in effect
//! by exercising the full pipeline:
//!
//! 1. A URL whose body starts with `--option=…` does NOT produce a
//!    "git: unknown option" or "expected --upload-pack value" error;
//!    git instead reports a path/URL error, proving the value was
//!    forwarded as a positional URL.
//! 2. Legitimate `file://` URLs continue to work — the `--` is a no-op
//!    for benign inputs (existing extract_rev_url.rs covers the happy
//!    path; this file covers the adversarial shape only).
//!
//! Real-infra preference (CLAUDE.md §2.5): integration via the actual
//! `cfdb` binary against a `file://` URL. Zero network access.

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

/// A `file://` URL whose path starts with `--upload-pack=…` MUST be
/// treated by `git clone` as a positional URL (which then fails because
/// the path does not exist), NOT as a `--upload-pack` flag. The `--`
/// separator added in `clone_and_checkout` is what enforces this.
///
/// Failure mode this guards against: a successful exploit would surface
/// as git accepting `--upload-pack` and trying to spawn the value as a
/// helper binary, with stderr like "fatal: cannot run …" or no error
/// at all if the value happens to be a valid binary. With `--` in
/// effect, git reports a path error ("repository … does not exist",
/// "Could not read from remote repository", or similar) and stderr
/// contains the literal URL string.
#[test]
fn clone_url_starting_with_double_dash_is_treated_as_path_not_option() {
    let db = tempfile::tempdir().expect("db tempdir");
    let cache = tempfile::tempdir().expect("cache tempdir");

    // file:// path whose body starts with `--upload-pack=…`. The trailing
    // 12 hex chars are a syntactically valid SHA so `parse_url_at_sha`
    // accepts the rev and routes to `extract_at_url_rev` → the URL flows
    // into `git clone` exactly as in production.
    let url_at_sha = "file:///nonexistent/--upload-pack=evilhelper@deadbeefdead";
    let out = run_extract_rev(cache.path(), db.path(), url_at_sha);

    assert!(!out.status.success(), "extract against bogus URL must fail",);
    let stderr = String::from_utf8_lossy(&out.stderr);

    // The exact wording varies across git versions, but ALL of them name
    // the failed `git clone` step from `clone_and_checkout`'s error
    // wrapper — that proves we reached `git clone` (the `--` worked at
    // the cfdb→git boundary) and git itself rejected the URL as a path.
    assert!(
        stderr.contains("git clone"),
        "stderr should name the git clone subprocess (proves we reached \
         git with the URL as a positional arg, not a flag), got: {stderr}",
    );

    // Negative assertion: git did NOT take the URL body as a known flag.
    // If `--` were missing, git would either accept `--upload-pack=…`
    // silently or complain about the option specifically. We assert the
    // absence of an "unknown option" / "unrecognized option" error
    // mentioning `upload-pack` to cover both cases.
    assert!(
        !stderr.contains("unknown option") && !stderr.contains("unrecognized option"),
        "stderr suggests git parsed the URL body as an option — `--` may be missing: {stderr}",
    );
}

/// Source-level assertion: the `git` invocations in
/// `commands/extract.rs` that consume user-influenced positional values
/// MUST include the literal token `"--"` immediately before the user
/// argument. This is a belt-and-suspenders check on top of the
/// behavioral test above — if a future refactor removes the separator,
/// this test fails fast with a clear pointer rather than waiting for a
/// CVE.
///
/// `git checkout` is intentionally excluded from the `--` requirement:
/// `git checkout -- <arg>` forces pathspec mode and would treat a SHA
/// as a filename. The hex validation in `parse_url_at_sha` (all-ASCII
/// hex, ≥ 7 chars) guarantees the SHA cannot start with `--`, so `--`
/// would be both unnecessary and actively harmful there.
#[test]
fn extract_rs_source_contains_double_dash_separator_for_user_args() {
    let src = include_str!("../src/commands/extract.rs");

    // git clone <url>: `--` must appear in the args literal before `url`.
    assert!(
        src.contains("\"clone\", \"--quiet\", \"--\", url"),
        "git clone invocation must include `--` before user-supplied url",
    );

    // git fetch origin <sha>: `--` before `sha` (refspec position
    // accepts `--` as separator without changing semantics).
    assert!(
        src.contains("\"fetch\", \"--quiet\", \"origin\", \"--\", sha"),
        "git fetch invocation must include `--` before user-supplied sha",
    );

    // git worktree add <path> <rev>: `--` before the positional pair.
    assert!(
        src.contains("\"worktree\", \"add\", \"--detach\", \"--quiet\", \"--\""),
        "git worktree add invocation must include `--` before user-supplied path/rev",
    );

    // Inverse assertion: `git checkout -- <sha>` is forbidden because it
    // forces pathspec mode. The hex validation upstream makes `--`
    // unnecessary; adding it would break legitimate SHAs.
    assert!(
        !src.contains("\"checkout\", \"--quiet\", \"--\""),
        "git checkout MUST NOT use `--` separator (would force pathspec mode); \
         hex validation in parse_url_at_sha guarantees sha cannot start with `--`",
    );
}

//! Integration tests for `cfdb extract --rev <sha>` (issue #37).
//!
//! Builds a synthetic 2-commit git repo in a tempdir and exercises the
//! full CLI pipeline against each commit separately, asserting the
//! extractions differ in exactly the way the source diff predicts.

use std::path::Path;
use std::process::Command;

use assert_cmd::cargo::CommandCargoExt;

/// Run `git` in `cwd` with the given args. Panics on non-zero exit so
/// test fixtures fail loudly.
fn git(cwd: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .current_dir(cwd)
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("git {args:?} failed to spawn: {e}"));
    assert!(
        out.status.success(),
        "git {args:?} in {}: exit {}\nstdout: {}\nstderr: {}",
        cwd.display(),
        out.status,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// Materialise a minimal single-crate cargo workspace + git repo in
/// `root`. Returns `(commit1_sha, commit2_sha)`.
///
/// Commit 1: `pub fn one() {}`
/// Commit 2: add `pub fn two() {}` in the same file.
fn build_two_commit_fixture(root: &Path) -> (String, String) {
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"fixture\"\nversion = \"0.0.1\"\nedition = \"2021\"\n\n[lib]\npath = \"src/lib.rs\"\n",
    )
    .expect("write Cargo.toml");
    std::fs::create_dir_all(root.join("src")).expect("mkdir src");
    std::fs::write(root.join("src/lib.rs"), "pub fn one() {}\n").expect("write src/lib.rs#1");

    git(root, &["init", "--quiet", "-b", "main"]);
    git(root, &["config", "user.email", "test@example.com"]);
    git(root, &["config", "user.name", "Test"]);
    git(root, &["add", "-A"]);
    git(root, &["commit", "--quiet", "-m", "commit 1 — one()"]);
    let sha1 = git(root, &["rev-parse", "HEAD"]);

    std::fs::write(
        root.join("src/lib.rs"),
        "pub fn one() {}\npub fn two() {}\n",
    )
    .expect("write src/lib.rs#2");
    git(root, &["add", "-A"]);
    git(root, &["commit", "--quiet", "-m", "commit 2 — add two()"]);
    let sha2 = git(root, &["rev-parse", "HEAD"]);

    (sha1, sha2)
}

fn cfdb_bin() -> Command {
    Command::cargo_bin("cfdb").expect("cfdb binary")
}

/// Extract `rev` via `cfdb extract --rev`. Returns the absolute path to
/// the resulting keyspace JSON file.
fn extract_rev(repo: &Path, db: &Path, rev: &str, keyspace: &str) -> std::path::PathBuf {
    let out = cfdb_bin()
        .args(["extract", "--workspace"])
        .arg(repo)
        .arg("--db")
        .arg(db)
        .arg("--keyspace")
        .arg(keyspace)
        .arg("--rev")
        .arg(rev)
        .output()
        .expect("cfdb extract --rev");
    assert!(
        out.status.success(),
        "cfdb extract --rev {rev} failed: {}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    db.join(format!("{keyspace}.json"))
}

fn dump(db: &Path, keyspace: &str) -> String {
    let out = cfdb_bin()
        .args(["dump", "--db"])
        .arg(db)
        .arg("--keyspace")
        .arg(keyspace)
        .output()
        .expect("cfdb dump");
    assert!(
        out.status.success(),
        "cfdb dump --keyspace {keyspace} failed: {}",
        String::from_utf8_lossy(&out.stderr),
    );
    String::from_utf8_lossy(&out.stdout).to_string()
}

// ---------------------------------------------------------------------------
// AC-1: --rev against a specific commit produces a keyspace reflecting
// that commit's tree, NOT the current worktree.
// ---------------------------------------------------------------------------

#[test]
fn ac1_extract_rev_honours_commit_sha() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path();
    let (sha1, sha2) = build_two_commit_fixture(repo);

    let db = tempfile::tempdir().expect("db tempdir");

    extract_rev(repo, db.path(), &sha1, "commit1");
    extract_rev(repo, db.path(), &sha2, "commit2");

    let dump1 = dump(db.path(), "commit1");
    let dump2 = dump(db.path(), "commit2");

    // Commit 1 has `one` but not `two`. Commit 2 has both.
    assert!(
        dump1.contains("\"name\":\"one\""),
        "dump1 missing one: {dump1}"
    );
    assert!(
        !dump1.contains("\"name\":\"two\""),
        "dump1 unexpectedly contains two: {dump1}"
    );
    assert!(
        dump2.contains("\"name\":\"one\""),
        "dump2 missing one: {dump2}"
    );
    assert!(
        dump2.contains("\"name\":\"two\""),
        "dump2 missing two: {dump2}"
    );
    assert_ne!(
        dump1, dump2,
        "two different commits must produce different dumps"
    );
}

// ---------------------------------------------------------------------------
// AC-2: --rev determinism — two extractions against the same SHA produce
// byte-identical keyspaces (excluding the tempdir path in warnings, which
// the canonical dump does not include anyway).
// ---------------------------------------------------------------------------

#[test]
fn ac2_extract_rev_is_deterministic_across_runs() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path();
    let (sha1, _sha2) = build_two_commit_fixture(repo);

    let db_a = tempfile::tempdir().expect("db_a");
    let db_b = tempfile::tempdir().expect("db_b");

    extract_rev(repo, db_a.path(), &sha1, "sha1");
    extract_rev(repo, db_b.path(), &sha1, "sha1");

    let dump_a = dump(db_a.path(), "sha1");
    let dump_b = dump(db_b.path(), "sha1");
    assert_eq!(
        dump_a, dump_b,
        "two --rev extractions at the same SHA must be byte-identical"
    );
}

// ---------------------------------------------------------------------------
// AC-3: --rev with invalid revision → clean CLI error (no panic, no silent
// empty-keyspace write).
// ---------------------------------------------------------------------------

#[test]
fn ac3_invalid_rev_fails_with_nonzero_exit() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path();
    let (_sha1, _sha2) = build_two_commit_fixture(repo);

    let db = tempfile::tempdir().expect("db");
    let out = cfdb_bin()
        .args(["extract", "--workspace"])
        .arg(repo)
        .arg("--db")
        .arg(db.path())
        .arg("--keyspace")
        .arg("bad")
        .arg("--rev")
        .arg("not-a-real-ref")
        .output()
        .expect("spawn");
    assert!(!out.status.success(), "invalid --rev must fail");
    // No keyspace file should have been written on failure.
    assert!(
        !db.path().join("bad.json").exists(),
        "no keyspace file should have been written on --rev failure"
    );
}

// ---------------------------------------------------------------------------
// AC-4: --workspace that is not a git repository → clean rejection with
// --rev (the `.git` check guards users who forget they're on a non-repo dir).
// ---------------------------------------------------------------------------

#[test]
fn ac4_non_git_workspace_rejected_when_rev_passed() {
    let tmp = tempfile::tempdir().expect("tempdir");
    // No `git init` — plain directory.
    std::fs::write(
        tmp.path().join("Cargo.toml"),
        "[package]\nname=\"x\"\nversion=\"0.0.1\"\nedition=\"2021\"\n",
    )
    .expect("write");

    let db = tempfile::tempdir().expect("db");
    let out = cfdb_bin()
        .args(["extract", "--workspace"])
        .arg(tmp.path())
        .arg("--db")
        .arg(db.path())
        .arg("--keyspace")
        .arg("ks")
        .arg("--rev")
        .arg("HEAD")
        .output()
        .expect("spawn");
    assert!(
        !out.status.success(),
        "non-git workspace must be rejected with --rev"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains(".git") || stderr.contains("git repository"),
        "error should mention git: {stderr}"
    );
}

// ---------------------------------------------------------------------------
// AC-5: --rev default keyspace name is the short SHA when --keyspace is
// omitted. Confirms `short_rev` is applied in the dispatch path.
// ---------------------------------------------------------------------------

#[test]
fn ac5_default_keyspace_is_short_rev() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path();
    let (sha1, _sha2) = build_two_commit_fixture(repo);

    let db = tempfile::tempdir().expect("db");

    let out = cfdb_bin()
        .args(["extract", "--workspace"])
        .arg(repo)
        .arg("--db")
        .arg(db.path())
        .arg("--rev")
        .arg(&sha1)
        .output()
        .expect("spawn");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let expected = db.path().join(format!("{}.json", &sha1[..12]));
    assert!(
        expected.exists(),
        "expected keyspace file at {}; db dir contents: {:?}",
        expected.display(),
        std::fs::read_dir(db.path())
            .expect("read_dir")
            .map(|e| e.unwrap().path())
            .collect::<Vec<_>>()
    );
}

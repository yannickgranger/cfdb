use std::path::{Path, PathBuf};
use std::process::Command;

use assert_cmd::cargo::CommandCargoExt;

fn git(cwd: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .current_dir(cwd)
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("git {args:?} failed to spawn: {e}"));
    assert!(
        out.status.success(),
        "git {args:?} in {}: exit {}\nstderr: {}",
        cwd.display(),
        out.status,
        String::from_utf8_lossy(&out.stderr),
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn build_bare_repo(tmp: &Path) -> (PathBuf, String, String) {
    let work = tmp.join("work");
    std::fs::create_dir_all(&work).expect("mkdir work");
    std::fs::write(
        work.join("Cargo.toml"),
        "[package]\nname = \"fixture\"\nversion = \"0.0.1\"\nedition = \"2021\"\n\n[lib]\npath = \"src/lib.rs\"\n",
    ).expect("write Cargo.toml");
    std::fs::create_dir_all(work.join("src")).expect("mkdir src");
    std::fs::write(work.join("src/lib.rs"), "pub fn one() {}\n").expect("write src/lib.rs#1");

    git(&work, &["init", "--quiet", "-b", "main"]);
    git(&work, &["config", "user.email", "t@e.com"]);
    git(&work, &["config", "user.name", "Test"]);
    git(&work, &["add", "-A"]);
    git(&work, &["commit", "--quiet", "-m", "commit 1 — one()"]);
    let sha1 = git(&work, &["rev-parse", "HEAD"]);

    std::fs::write(
        work.join("src/lib.rs"),
        "pub fn one() {}\npub fn two() {}\n",
    )
    .expect("write src/lib.rs#2");
    git(&work, &["add", "-A"]);
    git(&work, &["commit", "--quiet", "-m", "commit 2 — add two()"]);
    let sha2 = git(&work, &["rev-parse", "HEAD"]);

    let bare = tmp.join("remote.git");
    let clone_out = Command::new("git")
        .args(["clone", "--quiet", "--bare"])
        .arg(&work)
        .arg(&bare)
        .output()
        .expect("git clone --bare");
    assert!(
        clone_out.status.success(),
        "git clone --bare failed: {}",
        String::from_utf8_lossy(&clone_out.stderr)
    );
    git(
        &bare,
        &["config", "uploadpack.allowReachableSHA1InWant", "true"],
    );

    (bare, sha1, sha2)
}

fn cfdb_bin() -> Command {
    Command::cargo_bin("cfdb").expect("cfdb binary")
}

fn extract_url_rev(
    cache: &Path,
    db: &Path,
    keyspace: Option<&str>,
    url_at_sha: &str,
) -> std::process::Output {
    let mut cmd = cfdb_bin();
    cmd.env("CFDB_CACHE_DIR", cache)
        .args(["extract", "--workspace"])
        .arg(cache)
        .arg("--db")
        .arg(db);
    if let Some(ks) = keyspace {
        cmd.arg("--keyspace").arg(ks);
    }
    cmd.arg("--rev")
        .arg(url_at_sha)
        .output()
        .expect("cfdb extract --rev <url>@<sha>")
}

#[test]
fn ac1_url_rev_honours_commit_sha() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (bare, sha1, sha2) = build_bare_repo(tmp.path());
    let db = tempfile::tempdir().expect("db tempdir");
    let cache = tempfile::tempdir().expect("cache tempdir");
    let file_url = format!("file://{}", bare.display());

    let at_sha1 = format!("{file_url}@{sha1}");
    let o1 = extract_url_rev(cache.path(), db.path(), Some("c1"), &at_sha1);
    assert!(
        o1.status.success(),
        "extract sha1 failed: stdout={} stderr={}",
        String::from_utf8_lossy(&o1.stdout),
        String::from_utf8_lossy(&o1.stderr),
    );
    let ks1 = std::fs::read_to_string(db.path().join("c1.json")).expect("c1.json");
    assert!(
        ks1.contains("\"one\""),
        "commit1 keyspace should contain one(): {ks1}"
    );
    assert!(
        !ks1.contains("\"two\""),
        "commit1 keyspace should NOT contain two() — commit 1 predates it: {ks1}"
    );

    let at_sha2 = format!("{file_url}@{sha2}");
    let o2 = extract_url_rev(cache.path(), db.path(), Some("c2"), &at_sha2);
    assert!(
        o2.status.success(),
        "extract sha2 failed: stdout={} stderr={}",
        String::from_utf8_lossy(&o2.stdout),
        String::from_utf8_lossy(&o2.stderr),
    );
    let ks2 = std::fs::read_to_string(db.path().join("c2.json")).expect("c2.json");
    assert!(
        ks2.contains("\"one\"") && ks2.contains("\"two\""),
        "commit2 keyspace should contain BOTH one() and two(): {ks2}"
    );
}

#[test]
fn ac3_second_run_hits_cache() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (bare, sha1, _) = build_bare_repo(tmp.path());
    let db = tempfile::tempdir().expect("db tempdir");
    let cache = tempfile::tempdir().expect("cache tempdir");
    let at_sha = format!("file://{}@{sha1}", bare.display());

    let o1 = extract_url_rev(cache.path(), db.path(), Some("c1"), &at_sha);
    assert!(o1.status.success(), "first extract failed");
    let stderr1 = String::from_utf8_lossy(&o1.stderr);
    assert!(
        stderr1.contains("cloning"),
        "first run should clone, stderr: {stderr1}"
    );

    let o2 = extract_url_rev(cache.path(), db.path(), Some("c1b"), &at_sha);
    assert!(o2.status.success(), "second extract failed");
    let stderr2 = String::from_utf8_lossy(&o2.stderr);
    assert!(
        stderr2.contains("cache hit"),
        "second run should hit cache, stderr: {stderr2}"
    );
    assert!(
        !stderr2.contains("cloning"),
        "second run should NOT re-clone, stderr: {stderr2}"
    );
}

#[test]
fn ac2_unreachable_url_surfaces_git_error() {
    let db = tempfile::tempdir().expect("db tempdir");
    let cache = tempfile::tempdir().expect("cache tempdir");
    let at_sha = "https://127.0.0.1:1/nonexistent.git@deadbeef";
    let out = extract_url_rev(cache.path(), db.path(), Some("doomed"), at_sha);
    assert!(!out.status.success(), "unreachable URL must fail");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("git clone"),
        "stderr should name the failed subprocess, got: {stderr}"
    );
}

#[test]
fn malformed_url_at_sha_falls_through_to_same_repo_path() {
    let db = tempfile::tempdir().expect("db tempdir");
    let cache = tempfile::tempdir().expect("cache tempdir");
    let at_sha = "http://host.com/r@notahex";
    let out = extract_url_rev(cache.path(), db.path(), Some("bad"), at_sha);
    assert!(!out.status.success(), "malformed URL@SHA must fail");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("--rev requires --workspace to point at a git repository root"),
        "malformed URL@SHA should fall through to extract_at_rev's repo-root error, got: {stderr}"
    );
}

#[test]
fn default_keyspace_is_short_rev_of_sha() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (bare, sha1, _) = build_bare_repo(tmp.path());
    let db = tempfile::tempdir().expect("db tempdir");
    let cache = tempfile::tempdir().expect("cache tempdir");
    let at_sha = format!("file://{}@{sha1}", bare.display());

    let out = extract_url_rev(cache.path(), db.path(), None, &at_sha);
    assert!(
        out.status.success(),
        "extract failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let expected_ks = &sha1[..12];
    let expected_path = db.path().join(format!("{expected_ks}.json"));
    assert!(
        expected_path.exists(),
        "expected keyspace file {} not found",
        expected_path.display()
    );
}

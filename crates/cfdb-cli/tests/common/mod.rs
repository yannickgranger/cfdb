//! Shared on-disk extract cache for cfdb-cli self-dogfood integration tests.
//!
//! Several self-dogfood tests (`classify_self_dogfood`, `scope`,
//! `classifier_taxonomy`) build the *same* keyspace — a `cfdb extract`
//! (+ enrich) of a fixed workspace — once per `#[test]`. Those extracts
//! dominate CI wall-clock (a single whole-tree extract is ~15–20s; ×N
//! tests ×2 keyspaces stacks into minutes).
//!
//! `cargo test` runs a binary's tests as threads in one process, so an
//! in-process `OnceLock` could share the build there — but `cargo
//! nextest` runs each test in its **own process**, where no in-process
//! cache survives. So the cache lives **on disk**: the first process to
//! claim an `O_EXCL` build lock runs `build`, writes the keyspace JSON,
//! then drops a `.ready` marker; concurrent processes spin until
//! `.ready` appears and reuse the same directory read-only.
//!
//! Faithfulness: the cached keyspace is byte-identical to what each test
//! would have extracted (same binary, same tree, deterministic extract),
//! so assertions are unchanged — only the *number* of extracts drops.
//!
//! Cache key = `label` + a fingerprint of the built `cfdb` binary, so a
//! recompiled binary invalidates the cache. In CI the cache dir does not
//! exist on a fresh checkout, so every run builds exactly once.

#![allow(dead_code)] // each test binary uses a different subset of helpers

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant, SystemTime};
use std::{fs, thread};

use assert_cmd::prelude::*;

/// cfdb sub-workspace root (parent of `crates/`).
pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/ parent")
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

/// Fingerprint the built `cfdb` binary by mtime — cargo rewrites the file
/// on every rebuild, so this invalidates the cache whenever the extractor
/// changes. Cheap (no hashing of the whole tree).
fn cfdb_fingerprint() -> u128 {
    let bin = assert_cmd::cargo::cargo_bin("cfdb");
    fs::metadata(&bin)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

/// A locked, on-disk cache directory built once and shared read-only
/// across every test process in the run.
///
/// `build(dir)` is invoked exactly once (under an `O_EXCL` lock) to
/// populate `dir` — typically one or more `cfdb extract`/`enrich`
/// invocations writing `<keyspace>.json` files into `dir`. Returns the
/// populated cache directory; callers point `cfdb --db <dir>` at it and
/// write any per-test outputs (diff envelopes, scope dumps) into their
/// own tempdir, never into `dir`.
pub fn cached_db(label: &str, build: impl FnOnce(&Path)) -> PathBuf {
    let root = workspace_root().join("target/test-fixtures");
    let key = format!("{label}-{}", cfdb_fingerprint());
    let dir = root.join(&key);
    let ready = dir.join(".ready");
    let lock = root.join(format!("{key}.lock"));

    fs::create_dir_all(&root).expect("mkdir test-fixtures root");
    let deadline = Instant::now() + Duration::from_secs(600);
    loop {
        if ready.exists() {
            return dir;
        }
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock)
        {
            Ok(_) => {
                // We own the build: rebuild the dir from scratch, run the
                // closure, then publish `.ready` and release the lock.
                let _ = fs::remove_dir_all(&dir);
                fs::create_dir_all(&dir).expect("mkdir cache dir");
                build(&dir);
                fs::write(&ready, b"ok").expect("write ready marker");
                let _ = fs::remove_file(&lock);
                return dir;
            }
            Err(_) => {
                // Another process is building. Steal a lock left by a
                // crashed builder (no `.ready` and lock older than 5 min).
                if let Ok(meta) = fs::metadata(&lock) {
                    if let Ok(age) = meta.modified().map(|t| t.elapsed().unwrap_or_default()) {
                        if age > Duration::from_secs(300) && !ready.exists() {
                            let _ = fs::remove_file(&lock);
                        }
                    }
                }
                assert!(
                    Instant::now() < deadline,
                    "timed out (600s) waiting for shared fixture `{key}` to build"
                );
                thread::sleep(Duration::from_millis(100));
            }
        }
    }
}

/// `cfdb extract` returning the raw `Output` for callers that assert on
/// stderr/exit (RFC-054 54-A dogfood). Same argv construction as
/// [`extract`], which delegates here.
pub fn extract_output(
    db_dir: &Path,
    workspace: &Path,
    keyspace: &str,
    extra_args: &[&str],
) -> std::process::Output {
    let mut args = vec![
        "extract",
        "--workspace",
        workspace.to_str().expect("workspace utf-8"),
        "--db",
        db_dir.to_str().expect("db utf-8"),
        "--keyspace",
        keyspace,
    ];
    args.extend_from_slice(extra_args);
    Command::cargo_bin("cfdb")
        .expect("cfdb binary built")
        .args(&args)
        .output()
        .expect("spawn `cfdb extract`")
}

/// `cfdb extract` `workspace` into `db_dir/<keyspace>.json`. `extra_args`
/// appends flags such as `--hir` / `--no-proc-macro`. Panics on failure;
/// use [`extract_output`] to assert on stderr/exit yourself.
pub fn extract(db_dir: &Path, workspace: &Path, keyspace: &str, extra_args: &[&str]) {
    let out = extract_output(db_dir, workspace, keyspace, extra_args);
    assert!(
        out.status.success(),
        "extract failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant, SystemTime};
use std::{fs, thread};

use assert_cmd::prelude::*;

pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/ parent")
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

fn cfdb_fingerprint() -> u128 {
    let bin = assert_cmd::cargo::cargo_bin("cfdb");
    fs::metadata(&bin)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

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
                let _ = fs::remove_dir_all(&dir);
                fs::create_dir_all(&dir).expect("mkdir cache dir");
                build(&dir);
                fs::write(&ready, b"ok").expect("write ready marker");
                let _ = fs::remove_file(&lock);
                return dir;
            }
            Err(_) => {
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

pub fn extract(db_dir: &Path, workspace: &Path, keyspace: &str, extra_args: &[&str]) {
    let out = extract_output(db_dir, workspace, keyspace, extra_args);
    assert!(
        out.status.success(),
        "extract failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

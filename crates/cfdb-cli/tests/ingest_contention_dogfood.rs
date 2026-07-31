//! RFC-054 54-A (#556) inject-bite — prove `cfdb extract` makes identity
//! contention loud instead of silently dropping nodes (#542).
//!
//! Planted drift: a synthetic workspace with TWO `src/bin/*.rs` targets.
//! Both `fn main`s render into the package namespace (pre-54-B behavior),
//! collide on one `item:` id, and the loser is silently replaced — 54-A
//! makes that replace warn. The 1-bin control fixture proves the warning
//! is not vacuously firing on every extract.
//!
//! Fixture directories live under `CARGO_TARGET_TMPDIR` (never `/tmp` or
//! a `/cache`-routed TMPDIR) per the #526-class runner-tmp-cleaner fix.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use assert_cmd::prelude::*;

mod common;

fn fixture_root(name: &str) -> PathBuf {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("ingest-contention-556")
        .join(name);
    if root.exists() {
        fs::remove_dir_all(&root).expect("clean stale fixture dir");
    }
    fs::create_dir_all(&root).expect("create fixture dir");
    root
}

/// Write a one-package fixture workspace. `bins` empty ⇒ a default
/// `src/main.rs`; otherwise one `src/bin/<name>.rs` per entry — each with an
/// identical `fn main`, which is exactly the #542 collision shape.
///
/// The `[workspace]` table is load-bearing: the fixture lives inside cfdb's
/// own `target/`, and without it cargo-metadata would climb to cfdb's
/// workspace.
fn write_fixture(name: &str, bins: &[&str]) -> PathBuf {
    let root = fixture_root(name);
    fs::write(
        root.join("Cargo.toml"),
        format!(
            "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[workspace]\n"
        ),
    )
    .expect("write Cargo.toml");
    let src = root.join("src");
    fs::create_dir_all(&src).expect("mkdir src");
    fs::write(
        src.join("lib.rs"),
        "pub fn shared_helper() -> u32 {\n    1\n}\n",
    )
    .expect("write lib.rs");
    let main_body = format!("fn main() {{\n    println!(\"{{}}\", {name}::shared_helper());\n}}\n");
    if bins.is_empty() {
        fs::write(src.join("main.rs"), &main_body).expect("write main.rs");
    } else {
        fs::create_dir_all(src.join("bin")).expect("mkdir src/bin");
        for bin in bins {
            fs::write(src.join("bin").join(format!("{bin}.rs")), &main_body).expect("write bin");
        }
    }
    root
}

fn query_mains(db: &Path, keyspace: &str) -> std::process::Output {
    Command::cargo_bin("cfdb")
        .expect("cfdb binary built for integration tests")
        .args([
            "query",
            "--db",
            db.to_str().expect("utf-8 db path"),
            "--keyspace",
            keyspace,
            "MATCH (i:Item) WHERE i.name = 'main' RETURN i.qname",
        ])
        .output()
        .expect("spawn `cfdb query`")
}

#[test]
fn two_bin_contention_warns_on_extract_stderr_and_query_output() {
    let ws = write_fixture("twobins", &["alpha", "beta"]);
    let db = fixture_root("twobins-db");

    // Inject-bite half 1: extract exits 0 (diagnostic, not failure) and
    // surfaces the contention on stderr.
    let out = common::extract_output(&db, &ws, "twobins", &[]);
    assert!(
        out.status.success(),
        "extract must stay exit-0 on contention (diagnostic, not failure): {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("identity contention"),
        "extract stderr must surface the contention, got:\n{stderr}"
    );

    // Inject-bite half 2: a LATER `cfdb query` process (fresh load from the
    // persisted keyspace) still carries the warning in its result JSON.
    let q = query_mains(&db, "twobins");
    assert!(q.status.success());
    let stdout = String::from_utf8_lossy(&q.stdout);
    assert!(
        stdout.contains("IdentityContention"),
        "query result warnings must carry the persisted contention, got:\n{stdout}"
    );
}

#[test]
fn one_bin_control_extract_is_contention_silent() {
    let ws = write_fixture("onebin", &[]);
    let db = fixture_root("onebin-db");

    let out = common::extract_output(&db, &ws, "onebin", &[]);
    assert!(
        out.status.success(),
        "control extract failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("identity contention"),
        "vacuity guard: control fixture must not warn, got:\n{stderr}"
    );

    let q = query_mains(&db, "onebin");
    assert!(q.status.success());
    let stdout = String::from_utf8_lossy(&q.stdout);
    assert!(
        !stdout.contains("IdentityContention"),
        "vacuity guard: control query must carry no contention warning, got:\n{stdout}"
    );
}

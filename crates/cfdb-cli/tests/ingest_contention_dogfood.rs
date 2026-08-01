//! RFC-054 54-A (#556) inject-bite — prove `cfdb extract` makes identity
//! contention loud instead of silently dropping nodes (#542).
//!
//! Planted drift (updated with RFC-054 54-B, #557): cross-TARGET collisions
//! are retired by target-scoped identity, so the fixture plants the
//! collision class that legitimately persists — cfg-gated fn twins in ONE
//! file. syn walks both cfg branches; the twin `:Item`s share a file
//! (silent per the ratified rule) but their `:Param` children carry
//! differing `type_path` props with no `file` prop, so the full-equality
//! fallback flags them — the exact class behind cfdb-self's 18 findings.
//! The control fixture proves the warning is not vacuously firing.
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

/// One lib file with cfg-gated same-name fns whose params differ — the
/// post-54-B contention shape (cfg twins share qname, file, and target,
/// so their `:Param` children contend on the full-equality fallback).
///
/// The `[workspace]` table is load-bearing: the fixture lives inside
/// cfdb's own `target/`, and without it cargo-metadata would climb to
/// cfdb's workspace.
fn write_cfg_twin_fixture() -> PathBuf {
    let root = fixture_root("cfgtwins");
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"cfgtwins\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[workspace]\n",
    )
    .expect("write Cargo.toml");
    let src = root.join("src");
    fs::create_dir_all(&src).expect("mkdir src");
    fs::write(
        src.join("lib.rs"),
        r##"#[cfg(feature = "alt")]
pub fn dispatch(x: u32) -> u32 {
    x
}

#[cfg(not(feature = "alt"))]
pub fn dispatch(x: &str) -> u32 {
    x.len() as u32
}
"##,
    )
    .expect("write lib.rs");
    root
}

/// Control: one lib + one default bin, no homonyms anywhere — extraction
/// must stay contention-silent.
fn write_one_bin_control() -> PathBuf {
    let root = fixture_root("onebin");
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"onebin\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[workspace]\n",
    )
    .expect("write Cargo.toml");
    let src = root.join("src");
    fs::create_dir_all(&src).expect("mkdir src");
    fs::write(
        src.join("lib.rs"),
        "pub fn shared_helper() -> u32 {\n    1\n}\n",
    )
    .expect("write lib.rs");
    fs::write(
        src.join("main.rs"),
        "fn main() {\n    println!(\"{}\", onebin::shared_helper());\n}\n",
    )
    .expect("write main.rs");
    root
}

fn query_by_name(db: &Path, keyspace: &str, name: &str) -> std::process::Output {
    Command::cargo_bin("cfdb")
        .expect("cfdb binary built for integration tests")
        .args([
            "query",
            "--db",
            db.to_str().expect("utf-8 db path"),
            "--keyspace",
            keyspace,
            &format!("MATCH (i:Item) WHERE i.name = '{name}' RETURN i.qname"),
        ])
        .output()
        .expect("spawn `cfdb query`")
}

#[test]
fn cfg_twin_contention_warns_on_extract_stderr_and_query_output() {
    let ws = write_cfg_twin_fixture();
    let db = fixture_root("cfgtwins-db");

    // Inject-bite half 1: extract exits 0 (diagnostic, not failure) and
    // surfaces the contention on stderr.
    let out = common::extract_output(&db, &ws, "cfgtwins", &[]);
    assert!(
        out.status.success(),
        "extract must stay exit-0 on contention (diagnostic, not failure): {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("identity contention"),
        "extract stderr must surface the cfg-twin param contention, got:\n{stderr}"
    );

    // Inject-bite half 2: a LATER `cfdb query` process (fresh load from the
    // persisted keyspace) still carries the warning in its result JSON.
    let q = query_by_name(&db, "cfgtwins", "dispatch");
    assert!(q.status.success());
    let stdout = String::from_utf8_lossy(&q.stdout);
    assert!(
        stdout.contains("IdentityContention"),
        "query result warnings must carry the persisted contention, got:\n{stdout}"
    );
}

#[test]
fn one_bin_control_extract_is_contention_silent() {
    let ws = write_one_bin_control();
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

    let q = query_by_name(&db, "onebin", "main");
    assert!(q.status.success());
    let stdout = String::from_utf8_lossy(&q.stdout);
    assert!(
        !stdout.contains("IdentityContention"),
        "vacuity guard: control query must carry no contention warning, got:\n{stdout}"
    );
}

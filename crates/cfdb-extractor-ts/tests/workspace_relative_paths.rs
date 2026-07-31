//! Regression coverage for issue #540 — the TS producer's copy of the
//! #527 workspace-root class of bug.
//!
//! Two observable defects before the fix, both triggered by root
//! spellings `detect()` accepts but `Path::file_name()` cannot digest
//! (`.` — CI's literal `cfdb extract --workspace .` shape — or any
//! `..`-terminated form):
//!
//! 1. `derive_crate_name` silently falls back to `"ts_workspace"`, so
//!    the `:Crate` node, every `:Item.crate` prop and every qname in
//!    the keyspace carries the fallback instead of the directory name.
//! 2. `strip_prefix(workspace_root).unwrap_or(file_path)` ships an
//!    absolute path as if workspace-relative on any mismatch instead
//!    of erroring loudly (the #527 dead-fence class).
//!
//! The fix canonicalizes the root once at `produce()` entry
//! (`cfdb_lang::canonical_workspace_root`) and computes relative paths
//! through the loud shared helper (`cfdb_lang::workspace_relative`).
//!
//! `cargo test` runs with CWD at the crate manifest dir, so the
//! literal relative paths below are exactly the argument shapes the
//! bug class requires.

use std::path::Path;

use cfdb_core::fact::PropValue;
use cfdb_extractor_ts::TypeScriptProducer;
use cfdb_lang::LanguageProducer;

/// A `..`-terminated root is the in-test stand-in for CI's
/// `--workspace .`: `detect()` resolves it fine, but
/// `Path::file_name()` returns `None` on it, which is the exact
/// trigger of the silent `"ts_workspace"` crate-name fallback.
#[test]
fn dot_dot_terminated_root_still_names_the_crate_after_the_directory() {
    let root = Path::new("tests/fixtures/ts-minimal/src/..");
    assert!(
        root.join("tsconfig.json").is_file(),
        "fixture missing at {} — cargo test must run with CWD at the crate root",
        root.display()
    );

    let (nodes, _) = TypeScriptProducer
        .produce(root)
        .expect("produce on the fixture must succeed");

    let crate_node = nodes
        .iter()
        .find(|n| n.label.as_str() == "Crate")
        .expect("producer must emit a :Crate node");
    let name = match crate_node.props.get("name") {
        Some(PropValue::Str(s)) => s.as_str(),
        other => panic!(":Crate.name must be a string prop, got {other:?}"),
    };
    assert_eq!(
        name, "ts-minimal",
        "crate must be named after the canonical directory, not the \
         silent `ts_workspace` fallback"
    );
}

/// Relative root argument → every emitted `file` prop must be
/// workspace-relative. Pins the #527 contract on the TS producer.
#[test]
fn relative_workspace_argument_yields_workspace_relative_file_props() {
    let root = Path::new("tests/fixtures/ts-minimal");
    let (nodes, _) = TypeScriptProducer
        .produce(root)
        .expect("produce on the fixture must succeed");

    let mut checked = 0;
    for node in &nodes {
        if let Some(PropValue::Str(file)) = node.props.get("file") {
            checked += 1;
            assert!(
                !file.starts_with('/'),
                "absolute `file` prop leaked from a relative-root produce(): {file}"
            );
            assert!(
                !file.contains("fixtures"),
                "`file` prop still carries the workspace-root prefix: {file}"
            );
        }
    }
    assert!(
        checked > 0,
        "fixture must emit at least one file-carrying node"
    );
}

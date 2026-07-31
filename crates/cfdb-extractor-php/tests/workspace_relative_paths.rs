//! Regression coverage for issue #540 — the PHP producer's copy of the
//! #527 workspace-root class of bug.
//!
//! The PHP producer names its synthetic `:Crate` with a constant, so
//! unlike the TS producer it has no crate-name symptom. What it shares
//! is the `strip_prefix(workspace_root).unwrap_or(&path)` pattern: on
//! any mismatch an absolute path ships as if workspace-relative — the
//! #527 silently-dead-fence class. The fix canonicalizes the root once
//! at `produce()` entry and routes every relative-path computation
//! through the loud shared helper (`cfdb_lang::workspace_relative`),
//! converging all three producers on one resolver.
//!
//! This test pins the observable contract on both root spellings the
//! CLI can pass (plain relative, and the `..`-terminated stand-in for
//! CI's `--workspace .`): every emitted `file` prop is
//! workspace-relative, never absolute, never root-prefixed.

use std::path::Path;

use cfdb_core::fact::PropValue;
use cfdb_extractor_php::PhpProducer;
use cfdb_lang::LanguageProducer;

fn assert_all_file_props_relative(root: &Path) {
    let (nodes, _) = PhpProducer
        .produce(root)
        .expect("produce on the fixture must succeed");

    let mut checked = 0;
    for node in &nodes {
        if let Some(PropValue::Str(file)) = node.props.get("file") {
            checked += 1;
            assert!(
                !file.starts_with('/'),
                "absolute `file` prop leaked from produce({}): {file}",
                root.display()
            );
            assert!(
                !file.contains("fixtures"),
                "`file` prop still carries the workspace-root prefix \
                 from produce({}): {file}",
                root.display()
            );
        }
    }
    assert!(
        checked > 0,
        "fixture must emit at least one file-carrying node"
    );
}

#[test]
fn relative_workspace_argument_yields_workspace_relative_file_props() {
    let root = Path::new("tests/fixtures/php-calls");
    assert!(
        root.join("composer.json").is_file(),
        "fixture missing at {} — cargo test must run with CWD at the crate root",
        root.display()
    );
    assert_all_file_props_relative(root);
}

#[test]
fn dot_dot_terminated_root_yields_workspace_relative_file_props() {
    assert_all_file_props_relative(Path::new("tests/fixtures/php-calls/src/.."));
}

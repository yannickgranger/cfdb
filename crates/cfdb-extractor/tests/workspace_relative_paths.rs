//! Regression coverage for issue #527: an extraction invoked through a
//! relative `--workspace` argument (exactly the shape CI's
//! `cfdb extract --workspace .` passes) must still emit workspace-relative
//! `:File.path` facts.
//!
//! Before the fix, the workspace root was never canonicalized: `cargo
//! metadata` always returns absolute file paths, so a relative
//! `workspace_root` argument made every `strip_prefix` call miss, and the
//! extractor silently fell back to shipping the absolute path. Every
//! file-scoped fence anchored on a relative path became a silently dead
//! rule — zero rows forever, false success.
//!
//! `cargo test` runs test binaries with the working directory set to the
//! crate's manifest directory (`CARGO_MANIFEST_DIR`), so the literal
//! relative path below is exactly the kind of argument the bug required —
//! not an absolute path built via `env!("CARGO_MANIFEST_DIR")`.

use std::path::Path;

use cfdb_core::fact::PropValue;
use cfdb_core::schema::Label;
use cfdb_extractor::extract_workspace;

#[test]
fn relative_workspace_argument_yields_workspace_relative_file_paths() {
    let root = Path::new("tests/fixtures/workspace_relative_fixture");
    assert!(
        root.join("Cargo.toml").is_file(),
        "fixture missing at {} — cargo test must run with CWD at the crate root",
        root.display()
    );
    assert!(
        root.is_relative(),
        "the whole point of this test is a relative --workspace argument"
    );

    let (nodes, _edges) = extract_workspace(root).expect("extract via relative workspace arg");

    let file_nodes: Vec<_> = nodes
        .iter()
        .filter(|n| n.label.as_str() == Label::FILE)
        .collect();
    assert!(
        !file_nodes.is_empty(),
        "fixture should emit at least one :File node"
    );

    for f in &file_nodes {
        let path = f
            .props
            .get("path")
            .and_then(PropValue::as_str)
            .unwrap_or_else(|| panic!("{}: :File node missing `path` prop", f.id));
        assert!(
            !Path::new(path).is_absolute(),
            "{}: :File.path must be workspace-relative, got absolute path {path:?} — issue \
             #527: a relative --workspace argument (e.g. `.`) must not fall back to an \
             absolute path when strip_prefix misses",
            f.id
        );
    }
}

//! Issue #531 — pin the documented containment approximation of
//! `Manifest::from_crate_graph` (entry_point_emitter/framework.rs) as a
//! tested contract.
//!
//! The approximation, quoted from its own doc comment: *"a non-member
//! crate physically nested UNDER the workspace root but reached via a
//! member's path-dependency (a vendored or `exclude`d crate) passes
//! this containment check — path containment cannot see cargo's member
//! list (the true `is_member` bit never reaches the HIR layer)."*
//!
//! Observable consequence pinned here: the excluded crate's
//! dependencies leak into the RFC-049 §3.1 manifest gate, so a
//! framework detector activates for the whole workspace even though no
//! actual MEMBER declares the framework dependency. The fixture:
//!
//! ```text
//! root/
//!   Cargo.toml            members = ["app"], exclude = ["vendored"]
//!   app/                  member; path-dep on ../vendored; clap-derive
//!                         struct in src; NO clap dependency
//!   vendored/             excluded; nested under root; depends on the
//!                         stub `clap` — the only clap dep in the tree
//!   clap/                 empty stub, satisfies the name-only gate
//! ```
//!
//! Under true cargo member semantics the gate would stay CLOSED (no
//! member depends on clap) and `app`'s derive would emit nothing.
//! Under the documented approximation the nested `vendored` crate is
//! "contained", its `clap` dep opens the gate, and the `cli_command`
//! entry point IS emitted. This test asserts the approximation's
//! behavior on purpose — when the real `is_member` bit ever reaches
//! the HIR layer, this test goes red and its assertions flip to the
//! member-semantics expectations (that flip is the desired signal, not
//! a regression).

use std::fs;
use std::path::Path;

use cfdb_core::fact::{Node, PropValue};
use cfdb_core::schema::Label;
use cfdb_hir_extractor::{build_hir_database, extract_entry_points};
use tempfile::tempdir;

fn write(root: &Path, rel: &str, contents: &str) {
    let p = root.join(rel);
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent).expect("fixture mkdir -p");
    }
    fs::write(p, contents).expect("fixture write");
}

fn entry_points(nodes: &[Node]) -> Vec<&Node> {
    nodes
        .iter()
        .filter(|n| n.label.as_str() == Label::ENTRY_POINT)
        .collect()
}

fn kind_of(n: &Node) -> Option<&str> {
    n.props.get("kind").and_then(PropValue::as_str)
}

#[test]
fn nested_excluded_crate_deps_open_the_manifest_gate_known_approximation() {
    let tmp = tempdir().expect("tempdir");
    let root = tmp.path();

    write(
        root,
        "Cargo.toml",
        "[workspace]\nresolver = \"2\"\nmembers = [\n    \"app\"\n]\nexclude = [\n    \"vendored\",\n    \"clap\"\n]\n",
    );

    // Member `app`: reaches `vendored` via path-dep, declares NO clap
    // dependency itself, and carries the clap-derive idiom (the scan is
    // attribute-textual, so a local stand-in trait suffices — same
    // shape as tests/entry_point.rs).
    write(
        root,
        "app/Cargo.toml",
        "[package]\nname = \"app\"\nversion = \"0.0.1\"\nedition = \"2021\"\n\n[dependencies]\nvendored = { path = \"../vendored\" }\n",
    );
    write(
        root,
        "app/src/lib.rs",
        r#"
pub trait Parser {}

#[derive(Parser)]
pub struct Cli {
    pub arg: String,
}

pub fn run() -> Cli {
    Cli { arg: String::new() }
}
"#,
    );

    // Excluded-but-nested `vendored`: the only crate in the tree that
    // depends on (stub) clap.
    write(
        root,
        "vendored/Cargo.toml",
        "[package]\nname = \"vendored\"\nversion = \"0.0.1\"\nedition = \"2021\"\n\n[dependencies]\nclap = { path = \"../clap\" }\n",
    );
    write(root, "vendored/src/lib.rs", "");

    // Name-only stub clap (also excluded from the member list).
    write(
        root,
        "clap/Cargo.toml",
        "[package]\nname = \"clap\"\nversion = \"0.0.1\"\nedition = \"2021\"\n",
    );
    write(root, "clap/src/lib.rs", "");

    let (db, vfs, _proc_macro, targets) =
        build_hir_database(root, false).expect("build_hir_database on containment fixture");
    let (nodes, _edges) = extract_entry_points(&db, &vfs, root, &targets)
        .expect("extract_entry_points on containment fixture");

    let cli_eps: Vec<_> = entry_points(&nodes)
        .into_iter()
        .filter(|n| kind_of(n) == Some("cli_command"))
        .collect();

    // THE PIN (known approximation, on purpose): no workspace MEMBER
    // depends on clap — under true member semantics this list would be
    // empty — yet the nested-but-excluded `vendored` crate's clap dep
    // opens the manifest gate and the member's derive emits. If this
    // assertion goes red because the list became empty, the `is_member`
    // bit has reached the HIR layer: flip the expectation to
    // `cli_eps.is_empty()` and retire the approximation note in
    // `Manifest::from_crate_graph`.
    assert!(
        !cli_eps.is_empty(),
        "documented containment approximation no longer holds: the \
         nested-but-excluded crate's dependency stopped opening the \
         manifest gate — update framework.rs's approximation note and \
         flip this pin to member-semantics expectations"
    );
}

/// Negative control (vacuity guard): identical fixture minus the
/// `vendored → clap` dependency. With no clap dep anywhere in the tree
/// the manifest gate MUST stay closed and the member's derive emits
/// nothing — proving the pin above measures the gate, not an
/// always-emitting detector.
#[test]
fn without_the_excluded_crates_dep_the_manifest_gate_stays_closed() {
    let tmp = tempdir().expect("tempdir");
    let root = tmp.path();

    write(
        root,
        "Cargo.toml",
        "[workspace]\nresolver = \"2\"\nmembers = [\n    \"app\"\n]\nexclude = [\n    \"vendored\"\n]\n",
    );
    write(
        root,
        "app/Cargo.toml",
        "[package]\nname = \"app\"\nversion = \"0.0.1\"\nedition = \"2021\"\n\n[dependencies]\nvendored = { path = \"../vendored\" }\n",
    );
    write(
        root,
        "app/src/lib.rs",
        r#"
pub trait Parser {}

#[derive(Parser)]
pub struct Cli {
    pub arg: String,
}

pub fn run() -> Cli {
    Cli { arg: String::new() }
}
"#,
    );
    write(
        root,
        "vendored/Cargo.toml",
        "[package]\nname = \"vendored\"\nversion = \"0.0.1\"\nedition = \"2021\"\n",
    );
    write(root, "vendored/src/lib.rs", "");

    let (db, vfs, _proc_macro, targets) =
        build_hir_database(root, false).expect("build_hir_database on control fixture");
    let (nodes, _edges) = extract_entry_points(&db, &vfs, root, &targets)
        .expect("extract_entry_points on control fixture");

    let cli_eps: Vec<_> = entry_points(&nodes)
        .into_iter()
        .filter(|n| kind_of(n) == Some("cli_command"))
        .collect();

    assert!(
        cli_eps.is_empty(),
        "control fixture has no clap dependency anywhere, yet cli_command \
         entry points were emitted — the RFC-049 manifest gate is not \
         being consulted, which voids the containment pin"
    );
}

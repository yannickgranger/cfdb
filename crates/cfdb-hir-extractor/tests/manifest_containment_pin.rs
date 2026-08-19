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
        "[package]\nname = \"vendored\"\nversion = \"0.0.1\"\nedition = \"2021\"\n\n[dependencies]\nclap = { path = \"../clap\" }\n",
    );
    write(root, "vendored/src/lib.rs", "");

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

    assert!(
        !cli_eps.is_empty(),
        "documented containment approximation no longer holds: the \
         nested-but-excluded crate's dependency stopped opening the \
         manifest gate — update framework.rs's approximation note and \
         flip this pin to member-semantics expectations"
    );
}

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

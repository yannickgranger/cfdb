//! Self-dogfood test for `:Crate.published_language` prop emission.
//!
//! Asserts the end-to-end extract pipeline wires the loader output through
//! `emit_crate_and_walk_targets` onto every `:Crate` node's props —
//! guards against silent prop loss if `:Crate` emission is refactored.
//!
//! Uses a synthetic 2-crate workspace fixture in a tempdir with a 1-entry
//! `.cfdb/published-language-crates.toml`; assert one `:Crate` carries
//! `published_language: true` and the other `false`. Hermetic — no
//! dependency on cfdb's own tree state.

use cfdb_core::fact::PropValue;
use cfdb_core::schema::Label;

fn write_workspace_with_pl(tmp: &std::path::Path) {
    // Top-level workspace Cargo.toml listing two members.
    std::fs::write(
        tmp.join("Cargo.toml"),
        r#"[workspace]
resolver = "2"
members = ["qbot-prelude", "cfdb-local-crate"]
"#,
    )
    .expect("write workspace Cargo.toml");

    // Member 1: qbot-prelude (declared as Published Language)
    let prelude = tmp.join("qbot-prelude");
    std::fs::create_dir_all(prelude.join("src")).expect("mkdir qbot-prelude/src");
    std::fs::write(
        prelude.join("Cargo.toml"),
        r#"[package]
name = "qbot-prelude"
version = "0.0.1"
edition = "2021"

[lib]
path = "src/lib.rs"
"#,
    )
    .expect("write qbot-prelude Cargo.toml");
    std::fs::write(prelude.join("src").join("lib.rs"), "pub fn prelude() {}\n")
        .expect("write qbot-prelude lib.rs");

    // Member 2: cfdb-local-crate (NOT declared as Published Language)
    let local = tmp.join("cfdb-local-crate");
    std::fs::create_dir_all(local.join("src")).expect("mkdir cfdb-local-crate/src");
    std::fs::write(
        local.join("Cargo.toml"),
        r#"[package]
name = "cfdb-local-crate"
version = "0.0.1"
edition = "2021"

[lib]
path = "src/lib.rs"
"#,
    )
    .expect("write cfdb-local-crate Cargo.toml");
    std::fs::write(local.join("src").join("lib.rs"), "pub fn local() {}\n")
        .expect("write cfdb-local-crate lib.rs");

    // .cfdb/published-language-crates.toml — flags only qbot-prelude.
    let cfdb = tmp.join(".cfdb");
    std::fs::create_dir_all(&cfdb).expect("mkdir .cfdb");
    std::fs::write(
        cfdb.join("published-language-crates.toml"),
        r#"[[crate]]
name = "qbot-prelude"
language = "prelude"
owning_context = "core"
consumers = ["trading"]
"#,
    )
    .expect("write PL toml");
}

#[test]
fn extractor_emits_published_language_prop_on_every_crate_node() {
    let tmp = tempfile::tempdir().expect("tempdir");
    write_workspace_with_pl(tmp.path());

    let (nodes, _edges) = cfdb_extractor::extract_workspace(tmp.path()).expect("extract_workspace");

    // Collect every :Crate node, keyed by name.
    let crate_nodes: Vec<_> = nodes
        .iter()
        .filter(|n| n.label.as_str() == Label::CRATE)
        .collect();
    assert_eq!(
        crate_nodes.len(),
        2,
        "expected exactly 2 :Crate nodes, got {}",
        crate_nodes.len()
    );

    for node in &crate_nodes {
        let prop = node.props.get("published_language").unwrap_or_else(|| {
            panic!(
                "Crate node {} is missing `published_language` prop — \
                 every :Crate MUST carry this prop per issue #100 AC-6",
                node.id
            )
        });
        let name = match node.props.get("name") {
            Some(PropValue::Str(s)) => s.as_str(),
            other => panic!("Crate node {} has no name prop, got {other:?}", node.id),
        };

        match (name, prop) {
            ("qbot-prelude", PropValue::Bool(true)) => {} // expected
            ("cfdb-local-crate", PropValue::Bool(false)) => {} // expected
            (n, p) => panic!("unexpected (name, published_language) pair: ({n}, {p:?})"),
        }
    }
}

#[test]
fn extractor_emits_published_language_false_when_pl_file_absent() {
    let tmp = tempfile::tempdir().expect("tempdir");
    // Workspace WITHOUT a .cfdb/ directory — baseline case.
    std::fs::write(
        tmp.path().join("Cargo.toml"),
        r#"[workspace]
resolver = "2"
members = ["sole-crate"]
"#,
    )
    .expect("write workspace Cargo.toml");

    let sole = tmp.path().join("sole-crate");
    std::fs::create_dir_all(sole.join("src")).expect("mkdir sole-crate/src");
    std::fs::write(
        sole.join("Cargo.toml"),
        r#"[package]
name = "sole-crate"
version = "0.0.1"
edition = "2021"

[lib]
path = "src/lib.rs"
"#,
    )
    .expect("write sole-crate Cargo.toml");
    std::fs::write(sole.join("src").join("lib.rs"), "pub fn sole() {}\n")
        .expect("write sole-crate lib.rs");

    let (nodes, _edges) = cfdb_extractor::extract_workspace(tmp.path()).expect("extract_workspace");

    let crate_nodes: Vec<_> = nodes
        .iter()
        .filter(|n| n.label.as_str() == Label::CRATE)
        .collect();
    assert_eq!(crate_nodes.len(), 1);

    let prop = crate_nodes[0]
        .props
        .get("published_language")
        .expect("published_language prop must be present even without PL TOML");
    assert!(
        matches!(prop, PropValue::Bool(false)),
        "no PL file → published_language=false baseline, got {prop:?}"
    );
}

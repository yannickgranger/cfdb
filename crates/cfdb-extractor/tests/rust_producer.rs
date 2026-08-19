use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use cfdb_extractor::{extract_workspace, RustProducer};
use cfdb_lang::{LanguageError, LanguageProducer};
use tempfile::TempDir;

fn write_minimal_workspace(root: &Path) {
    fs::write(
        root.join("Cargo.toml"),
        r#"[workspace]
resolver = "2"
members = ["my_crate"]
"#,
    )
    .expect("write workspace Cargo.toml");
    fs::create_dir_all(root.join("my_crate/src")).expect("mkdir my_crate/src");
    fs::write(
        root.join("my_crate/Cargo.toml"),
        r#"[package]
name = "my_crate"
version = "0.0.1"
edition = "2021"
publish = false

[lib]
"#,
    )
    .expect("write my_crate Cargo.toml");
    fs::write(
        root.join("my_crate/src/lib.rs"),
        "pub fn hello(x: u32) -> u32 { x + 1 }\n",
    )
    .expect("write lib.rs");
}

#[test]
fn detect_returns_true_when_cargo_toml_present() {
    let dir = TempDir::new().expect("tempdir");
    write_minimal_workspace(dir.path());

    assert!(
        RustProducer.detect(dir.path()),
        "RustProducer.detect must return true on a workspace root carrying Cargo.toml"
    );
}

#[test]
fn detect_returns_false_when_cargo_toml_absent() {
    let dir = TempDir::new().expect("tempdir");
    fs::write(dir.path().join("random.txt"), "hello").expect("write random.txt");

    assert!(
        !RustProducer.detect(dir.path()),
        "RustProducer.detect must return false on a directory missing Cargo.toml"
    );
}

#[test]
fn detect_returns_false_when_cargo_toml_is_a_directory() {
    let dir = TempDir::new().expect("tempdir");
    fs::create_dir_all(dir.path().join("Cargo.toml")).expect("mkdir Cargo.toml");

    assert!(
        !RustProducer.detect(dir.path()),
        "RustProducer.detect must return false when `Cargo.toml` is a directory, not a file"
    );
}

#[test]
fn produce_and_extract_workspace_emit_byte_identical_facts() {
    let dir = TempDir::new().expect("tempdir");
    write_minimal_workspace(dir.path());

    let (legacy_nodes, legacy_edges) =
        extract_workspace(dir.path()).expect("legacy extract_workspace");
    let (trait_nodes, trait_edges) = RustProducer
        .produce(dir.path())
        .expect("RustProducer.produce");

    let legacy_node_ids: Vec<&str> = legacy_nodes.iter().map(|n| n.id.as_str()).collect();
    let trait_node_ids: Vec<&str> = trait_nodes.iter().map(|n| n.id.as_str()).collect();
    assert_eq!(legacy_node_ids, trait_node_ids, "node id sequences diverge");

    let legacy_edge_keys: Vec<(String, String, String)> = legacy_edges
        .iter()
        .map(|e| (e.src.clone(), e.dst.clone(), e.label.as_str().to_string()))
        .collect();
    let trait_edge_keys: Vec<(String, String, String)> = trait_edges
        .iter()
        .map(|e| (e.src.clone(), e.dst.clone(), e.label.as_str().to_string()))
        .collect();
    assert_eq!(
        legacy_edge_keys, trait_edge_keys,
        "edge tuple sequences diverge"
    );

    let legacy_props: BTreeMap<&str, &cfdb_core::fact::Props> = legacy_nodes
        .iter()
        .map(|n| (n.id.as_str(), &n.props))
        .collect();
    let trait_props: BTreeMap<&str, &cfdb_core::fact::Props> = trait_nodes
        .iter()
        .map(|n| (n.id.as_str(), &n.props))
        .collect();
    assert_eq!(legacy_props.len(), trait_props.len());
    for (id, legacy) in &legacy_props {
        let trait_bag = trait_props
            .get(id)
            .unwrap_or_else(|| panic!("trait path missing node {id}"));
        assert_eq!(
            *legacy, *trait_bag,
            "property bag mismatch for node {id}: legacy={legacy:?} trait={trait_bag:?}"
        );
    }
}

#[test]
fn produce_maps_extractor_failure_to_parse_variant() {
    let dir = TempDir::new().expect("tempdir");
    fs::write(dir.path().join("Cargo.toml"), "this is not valid toml = =")
        .expect("write malformed Cargo.toml");

    let err = RustProducer
        .produce(dir.path())
        .expect_err("malformed Cargo.toml must error");
    match err {
        LanguageError::Parse { producer, message } => {
            assert_eq!(producer, "rust", "producer name must propagate");
            assert!(
                !message.is_empty(),
                "Parse variant must carry the underlying ExtractError message"
            );
        }
        other => {
            panic!("expected LanguageError::Parse {{ producer: \"rust\", ... }}; got {other:?}")
        }
    }
}

#[test]
fn name_returns_canonical_rust_identifier() {
    assert_eq!(RustProducer.name(), "rust");
}

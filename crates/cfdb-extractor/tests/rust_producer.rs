//! `RustProducer` (RFC-041 Phase 1 / Slice 41-B) integration tests.
//!
//! Pin the LanguageProducer-side surface against the legacy
//! `extract_workspace` entry point so the trait method is provably a
//! no-op wrapper. Mirrors the regression-test pattern the
//! `param_emission` + `pattern_b_*` test suites use.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use cfdb_extractor::{extract_workspace, RustProducer};
use cfdb_lang::{LanguageError, LanguageProducer};
use tempfile::TempDir;

/// Build a synthetic single-crate workspace at `root` with one `lib.rs`
/// containing one fn — enough to exercise the extractor end-to-end.
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
    // No Cargo.toml — just an unrelated marker file.
    fs::write(dir.path().join("random.txt"), "hello").expect("write random.txt");

    assert!(
        !RustProducer.detect(dir.path()),
        "RustProducer.detect must return false on a directory missing Cargo.toml"
    );
}

#[test]
fn detect_returns_false_when_cargo_toml_is_a_directory() {
    let dir = TempDir::new().expect("tempdir");
    // Pathological case: `Cargo.toml` exists but as a DIRECTORY, not a
    // file. `is_file()` correctly returns false (not just `exists()`).
    fs::create_dir_all(dir.path().join("Cargo.toml")).expect("mkdir Cargo.toml");

    assert!(
        !RustProducer.detect(dir.path()),
        "RustProducer.detect must return false when `Cargo.toml` is a directory, not a file"
    );
}

/// `RustProducer.produce(ws)` and `extract_workspace(ws)` must produce
/// the same fact set on the same workspace — the trait method is a
/// no-op wrapper over the legacy entry point. Catches accidental
/// divergence in the `LanguageError` translation path or any future
/// refactor that breaks the equivalence.
#[test]
fn produce_and_extract_workspace_emit_byte_identical_facts() {
    let dir = TempDir::new().expect("tempdir");
    write_minimal_workspace(dir.path());

    let (legacy_nodes, legacy_edges) =
        extract_workspace(dir.path()).expect("legacy extract_workspace");
    let (trait_nodes, trait_edges) = RustProducer
        .produce(dir.path())
        .expect("RustProducer.produce");

    // Compare by canonical sorted-key shape: node ids sorted, edge
    // tuples sorted. The extractor already returns sorted output; this
    // assertion catches any drift.
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

    // Pin a couple of structural shape attributes so a regression in
    // node prop emission is caught here too — the diff would surface
    // in any property-bag drift between the two paths.
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

/// `produce()` propagates extractor failure as
/// `LanguageError::Parse { producer: "rust", ... }`, NOT as
/// `LanguageError::Io` or `NotDetected`. Run on a workspace whose
/// `Cargo.toml` is malformed enough that `cargo metadata` fails.
#[test]
fn produce_maps_extractor_failure_to_parse_variant() {
    let dir = TempDir::new().expect("tempdir");
    // Workspace Cargo.toml with invalid TOML — cargo_metadata will
    // surface a Metadata error which extract_workspace wraps in
    // `ExtractError::Metadata`.
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

/// Trait identity: `RustProducer.name()` returns the canonical
/// `"rust"` string the CLI dispatcher matches the `lang-rust` Cargo
/// feature against (RFC-041 §3.4 dispatch shape).
#[test]
fn name_returns_canonical_rust_identifier() {
    assert_eq!(RustProducer.name(), "rust");
}

//! `PhpProducer` (RFC-041 Phase 2 / issue #264) integration tests.
//!
//! Pin the `LanguageProducer`-side surface for the PHP MVP. Mirrors the
//! `cfdb-extractor`'s `tests/rust_producer.rs` shape: detection, name,
//! and end-to-end fact emission against a synthetic Composer fixture.
//!
//! AC bar (intentionally low for MVP — see issue #264):
//! - ≥ 1 namespace emitted as `:Module`
//! - ≥ 1 class/interface emitted as `:Item { kind: "trait" }`
//! - ≥ 1 method emitted as `:Item { kind: "fn" }`
//! - extract completes in < 5 s wall-clock on the fixture
//!
//! Schema-mapping rationale: PHP's `class` / `interface` / `trait` map
//! to `:Item { kind: "trait" }` because `:Item.kind` is closed-set in
//! `cfdb-core::schema::labels` and Phase 2 is not a schema-RFC slice.
//! See `cfdb-extractor-php/src/lib.rs` crate-root docs for the full
//! decision rationale and follow-up RFC pointer.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use cfdb_extractor_php::PhpProducer;
use cfdb_lang::LanguageProducer;
use tempfile::TempDir;

/// Path to the in-tree PHP fixture project. Resolved at test time
/// from `CARGO_MANIFEST_DIR` so the test passes regardless of how the
/// crate is built (workspace vs standalone).
fn fixture_root() -> PathBuf {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    Path::new(manifest_dir).join("tests/fixtures/php-minimal")
}

#[test]
fn name_returns_php_identifier() {
    // Pin the canonical `"php"` string the CLI dispatcher will gate
    // the `lang-php` Cargo feature against. Drift here would silently
    // break the registry lookup in `cfdb-cli`'s `available_producers()`.
    assert_eq!(PhpProducer.name(), "php");
}

#[test]
fn detect_returns_true_when_composer_json_present() {
    let dir = TempDir::new().expect("tempdir");
    fs::write(
        dir.path().join("composer.json"),
        r#"{"name":"acme/foo","type":"library"}"#,
    )
    .expect("write composer.json");

    assert!(
        PhpProducer.detect(dir.path()),
        "PhpProducer.detect must return true on a workspace root carrying composer.json"
    );
}

#[test]
fn detect_returns_false_when_composer_json_absent() {
    let dir = TempDir::new().expect("tempdir");
    // No composer.json — just an unrelated marker file.
    fs::write(dir.path().join("README.md"), "hello").expect("write README");

    assert!(
        !PhpProducer.detect(dir.path()),
        "PhpProducer.detect must return false on a directory missing composer.json"
    );
}

/// AC bar — extract the fixture and assert the closed-set mapping
/// produces at least the required count for each concept.
#[test]
fn produce_emits_at_least_one_namespace_one_class_one_method() {
    let root = fixture_root();
    let (nodes, edges) = PhpProducer
        .produce(&root)
        .expect("PhpProducer.produce on fixture");

    // ≥ 1 namespace → :Module
    let module_count = nodes
        .iter()
        .filter(|n| n.label.as_str() == "Module")
        .count();
    assert!(
        module_count >= 1,
        "expected ≥ 1 :Module node (PHP namespace); got {module_count}. nodes={:?}",
        node_summary(&nodes)
    );

    // ≥ 1 class/interface → :Item{kind:"trait"}
    let class_like_count = nodes
        .iter()
        .filter(|n| {
            n.label.as_str() == "Item"
                && n.props.get("kind").and_then(|v| v.as_str()) == Some("trait")
        })
        .count();
    assert!(
        class_like_count >= 1,
        "expected ≥ 1 :Item{{kind:\"trait\"}} (PHP class/interface); got {class_like_count}. nodes={:?}",
        node_summary(&nodes)
    );

    // ≥ 1 method → :Item{kind:"fn"}
    let fn_count = nodes
        .iter()
        .filter(|n| {
            n.label.as_str() == "Item" && n.props.get("kind").and_then(|v| v.as_str()) == Some("fn")
        })
        .count();
    assert!(
        fn_count >= 1,
        "expected ≥ 1 :Item{{kind:\"fn\"}} (PHP method/function); got {fn_count}. nodes={:?}",
        node_summary(&nodes)
    );

    // Sanity-check structural edges: at least one IN_CRATE and at
    // least one IN_MODULE (since the fixture has both a namespace
    // and items inside it).
    let in_crate = edges
        .iter()
        .filter(|e| e.label.as_str() == "IN_CRATE")
        .count();
    let in_module = edges
        .iter()
        .filter(|e| e.label.as_str() == "IN_MODULE")
        .count();
    assert!(in_crate >= 1, "expected ≥ 1 IN_CRATE edge; got {in_crate}");
    assert!(
        in_module >= 1,
        "expected ≥ 1 IN_MODULE edge; got {in_module}"
    );
}

/// AC bar — wall-clock budget. The MVP fixture is tiny (two files,
/// ~25 LOC total); the 5 s ceiling is a generous safety net against
/// catastrophic regressions (e.g. tree-sitter pathological re-parse,
/// directory-walk loops). Real-world Symfony-scale projects will be
/// re-budgeted in a follow-up.
#[test]
fn produce_under_5s_on_fixture() {
    let root = fixture_root();
    let start = Instant::now();
    let _ = PhpProducer
        .produce(&root)
        .expect("PhpProducer.produce on fixture");
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_secs_f64() < 5.0,
        "PhpProducer.produce must complete in < 5s on the MVP fixture; took {:.3}s",
        elapsed.as_secs_f64()
    );
}

/// Compact human-readable summary of (label, kind, name) for assertion
/// diagnostics. Avoids dumping the full `Node` debug shape (which
/// includes property bags) in the failure message.
fn node_summary(nodes: &[cfdb_core::fact::Node]) -> Vec<(String, Option<String>, Option<String>)> {
    nodes
        .iter()
        .map(|n| {
            (
                n.label.as_str().to_string(),
                n.props
                    .get("kind")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
                n.props
                    .get("name")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
            )
        })
        .collect()
}

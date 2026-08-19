use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use cfdb_extractor_php::PhpProducer;
use cfdb_lang::LanguageProducer;
use tempfile::TempDir;

fn fixture_root() -> PathBuf {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    Path::new(manifest_dir).join("tests/fixtures/php-minimal")
}

#[test]
fn name_returns_php_identifier() {
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
    fs::write(dir.path().join("README.md"), "hello").expect("write README");

    assert!(
        !PhpProducer.detect(dir.path()),
        "PhpProducer.detect must return false on a directory missing composer.json"
    );
}

#[test]
fn produce_emits_at_least_one_namespace_one_class_one_method() {
    let root = fixture_root();
    let (nodes, edges) = PhpProducer
        .produce(&root)
        .expect("PhpProducer.produce on fixture");

    let module_count = nodes
        .iter()
        .filter(|n| n.label.as_str() == "Module")
        .count();
    assert!(
        module_count >= 1,
        "expected ≥ 1 :Module node (PHP namespace); got {module_count}. nodes={:?}",
        node_summary(&nodes)
    );

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

use std::fs;

use cfdb_core::fact::{Node, PropValue};
use cfdb_core::schema::Label;
use cfdb_extractor_php::PhpProducer;
use cfdb_lang::LanguageProducer;
use tempfile::TempDir;

const COMPOSER_WITH_DEV_AUTOLOAD: &str = r#"{
  "name": "cfdb/test",
  "autoload": { "psr-4": { "App\\": "src/" } },
  "autoload-dev": { "psr-4": { "App\\Tests\\": "tests/" } }
}"#;

const COMPOSER_WITHOUT_DEV_AUTOLOAD: &str = r#"{"name":"cfdb/test","type":"library"}"#;

fn produce(composer: &str, files: &[(&str, &str)]) -> Vec<Node> {
    let dir = TempDir::new().expect("tempdir");
    fs::write(dir.path().join("composer.json"), composer).expect("write composer.json");
    for (rel, src) in files {
        let path = dir.path().join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("mkdir -p");
        }
        fs::write(&path, src).expect("write php source");
    }
    let (nodes, _) = PhpProducer
        .produce(dir.path())
        .expect("PhpProducer.produce");
    nodes
}

fn caller(body: &str) -> String {
    format!("<?php\nnamespace App;\nfunction f(): void {{\n{body}\n}}\n")
}

fn call_site_in<'a>(nodes: &'a [Node], file: &str) -> &'a Node {
    let matches: Vec<&Node> = nodes
        .iter()
        .filter(|n| n.label.as_str() == Label::CALL_SITE)
        .filter(|n| n.props.get("file").and_then(PropValue::as_str) == Some(file))
        .collect();
    assert_eq!(
        matches.len(),
        1,
        "expected exactly one :CallSite in {file}, got {}",
        matches.len(),
    );
    matches[0]
}

fn is_test(node: &Node) -> bool {
    match node.props.get("is_test") {
        Some(PropValue::Bool(b)) => *b,
        other => panic!("is_test must be a bool prop, got {other:?}"),
    }
}

#[test]
fn a_call_site_under_a_declared_dev_autoload_root_is_a_test_call_site() {
    let nodes = produce(
        COMPOSER_WITH_DEV_AUTOLOAD,
        &[
            ("src/Service.php", &caller("        strlen('a');")),
            ("tests/ServiceTest.php", &caller("        strlen('a');")),
        ],
    );
    assert!(
        !is_test(call_site_in(&nodes, "src/Service.php")),
        "a call site under the production autoload root is not a test call site"
    );
    assert!(
        is_test(call_site_in(&nodes, "tests/ServiceTest.php")),
        "a call site under a root composer.json declares in autoload-dev is a test call site"
    );
}

#[test]
fn a_dev_autoload_root_covers_every_depth_beneath_it() {
    let nodes = produce(
        COMPOSER_WITH_DEV_AUTOLOAD,
        &[(
            "tests/Support/Privacy/Cipher.php",
            &caller("        strlen('a');"),
        )],
    );
    assert!(
        is_test(call_site_in(&nodes, "tests/Support/Privacy/Cipher.php")),
        "a declared root covers the whole subtree, not only its immediate children"
    );
}

#[test]
fn a_workspace_declaring_no_dev_autoload_has_no_test_call_sites() {
    let nodes = produce(
        COMPOSER_WITHOUT_DEV_AUTOLOAD,
        &[("tests/ServiceTest.php", &caller("        strlen('a');"))],
    );
    assert!(
        !is_test(call_site_in(&nodes, "tests/ServiceTest.php")),
        "the test scope is the project's own declaration; a directory named tests is not one"
    );
}

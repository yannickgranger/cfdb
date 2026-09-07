use std::fs;

use cfdb_core::fact::{Edge, Node, PropValue};
use cfdb_extractor_php::PhpProducer;
use cfdb_lang::LanguageProducer;
use tempfile::TempDir;

const COMPOSER: &str = r#"{
  "name": "cfdb/test",
  "autoload": { "psr-4": { "App\\": "src/" } }
}"#;

const FIXTURE: &str = r#"<?php

namespace App;

use A\B\C;
use A\B\C as D;
use A\{B, C as E};
use \A\B;
use A\B\C, A\B\F;
use function A\f;
use const A\K;

class Fixture
{
}
"#;

const BRACED: &str = r#"<?php

namespace A {
    use X\Y;

    class Inner
    {
    }
}
"#;

const FIXTURE_FILE_ID: &str = "file:php-workspace:src/Fixture.php";
const BRACED_FILE_ID: &str = "file:php-workspace:src/Braced.php";

fn produce() -> (Vec<Node>, Vec<Edge>) {
    let dir = TempDir::new().expect("tempdir");
    fs::write(dir.path().join("composer.json"), COMPOSER).expect("write composer.json");
    fs::create_dir_all(dir.path().join("src")).expect("mkdir -p src");
    fs::write(dir.path().join("src/Fixture.php"), FIXTURE).expect("write Fixture.php");
    fs::write(dir.path().join("src/Braced.php"), BRACED).expect("write Braced.php");
    PhpProducer
        .produce(dir.path())
        .expect("PhpProducer.produce")
}

fn imports_of<'n>(nodes: &'n [Node], file: &str) -> Vec<&'n Node> {
    nodes
        .iter()
        .filter(|n| n.label.as_str() == "Import")
        .filter(|n| n.props.get("file").and_then(PropValue::as_str) == Some(file))
        .collect()
}

fn prop<'n>(node: &'n Node, key: &str) -> Option<&'n str> {
    node.props.get(key).and_then(PropValue::as_str)
}

fn line(node: &Node) -> i64 {
    match node.props.get("line") {
        Some(PropValue::Int(n)) => *n,
        other => panic!("`:Import.line` must be an Int, got {other:?}"),
    }
}

#[test]
fn every_class_importing_clause_is_one_import_node_with_its_own_id() {
    let (nodes, _) = produce();
    let imports = imports_of(&nodes, "src/Fixture.php");

    let observed: Vec<(&str, &str, Option<&str>, i64)> = imports
        .iter()
        .map(|n| {
            (
                n.id.as_str(),
                prop(n, "fqn").expect("`fqn` is mandatory on `:Import`"),
                prop(n, "alias"),
                line(n),
            )
        })
        .collect();

    assert_eq!(
        observed,
        vec![
            ("import:src/Fixture.php:A\\B:0", "A\\B", None, 7),
            ("import:src/Fixture.php:A\\B:1", "A\\B", None, 8),
            ("import:src/Fixture.php:A\\B\\C:0", "A\\B\\C", None, 5),
            ("import:src/Fixture.php:A\\B\\C:1", "A\\B\\C", Some("D"), 6),
            ("import:src/Fixture.php:A\\B\\C:2", "A\\B\\C", None, 9),
            ("import:src/Fixture.php:A\\B\\F:0", "A\\B\\F", None, 9),
            ("import:src/Fixture.php:A\\C:0", "A\\C", Some("E"), 7),
        ],
        "seven class-importing clauses, each its own node: the plain import, its aliased twin, \
         the two group members, the leading-backslash form, and the two of the comma-separated \
         declaration"
    );
}

#[test]
fn colliding_clauses_carry_distinct_ids_so_ingest_drops_none() {
    let (nodes, _) = produce();
    let imports = imports_of(&nodes, "src/Fixture.php");

    let mut ids: Vec<&str> = imports.iter().map(|n| n.id.as_str()).collect();
    let emitted = ids.len();
    ids.sort();
    ids.dedup();
    assert_eq!(
        ids.len(),
        emitted,
        "an id repeated across two clauses is overwritten by last-write-wins ingest and the \
         import is dropped in silence, so distinct ids — not the emitted count — are what this \
         asserts: {ids:?}"
    );

    let by_fqn = |fqn: &str| {
        imports
            .iter()
            .filter(|n| prop(n, "fqn") == Some(fqn))
            .count()
    };
    assert_eq!(by_fqn("A\\B\\C"), 3, "three clauses import `A\\B\\C`");
    assert_eq!(by_fqn("A\\B"), 2, "two clauses import `A\\B`");
}

#[test]
fn a_fqn_carries_no_leading_backslash_and_an_alias_is_never_case_folded() {
    let (nodes, _) = produce();
    let imports = imports_of(&nodes, "src/Fixture.php");

    for node in &imports {
        let fqn = prop(node, "fqn").expect("`fqn` is mandatory on `:Import`");
        assert!(
            !fqn.starts_with('\\'),
            "`use \\A\\B;` records the same name as `use A\\B;`, got {fqn:?}"
        );
    }

    let aliases: Vec<&str> = imports.iter().filter_map(|n| prop(n, "alias")).collect();
    assert_eq!(
        aliases,
        vec!["D", "E"],
        "the alias is the `as` name as written; the producer's internal table case-folds it and \
         the fact must not"
    );
}

#[test]
fn a_function_or_const_clause_is_not_a_class_import() {
    let (nodes, _) = produce();
    let fqns: Vec<&str> = imports_of(&nodes, "src/Fixture.php")
        .iter()
        .filter_map(|n| prop(n, "fqn"))
        .collect();
    assert!(
        !fqns.contains(&"A\\f") && !fqns.contains(&"A\\K"),
        "`use function` and `use const` import a symbol, not a class: {fqns:?}"
    );
}

#[test]
fn each_import_hangs_off_the_file_that_declares_it() {
    let (nodes, edges) = produce();
    let imports = imports_of(&nodes, "src/Fixture.php");

    let has_import: Vec<(&str, &str)> = edges
        .iter()
        .filter(|e| e.label.as_str() == "HAS_IMPORT")
        .map(|e| (e.src.as_str(), e.dst.as_str()))
        .collect();

    let expected: Vec<(&str, &str)> = imports
        .iter()
        .map(|n| (FIXTURE_FILE_ID, n.id.as_str()))
        .collect();
    assert_eq!(
        has_import, expected,
        "one HAS_IMPORT per :Import, every one from the declaring :File"
    );
}

#[test]
fn a_walked_file_is_a_node_carrying_only_what_a_producer_emits() {
    let (nodes, _) = produce();
    let file = nodes
        .iter()
        .find(|n| n.id == FIXTURE_FILE_ID)
        .expect("a walked PHP file is a `:File` node");

    assert_eq!(file.label.as_str(), "File");
    assert_eq!(prop(file, "path"), Some("src/Fixture.php"));
    assert_eq!(prop(file, "crate"), Some("php-workspace"));
    let mut keys: Vec<&str> = file.props.keys().map(String::as_str).collect();
    keys.sort();
    assert_eq!(
        keys,
        vec!["crate", "path"],
        "`loc` and `module_qpath` are declared and emitted by nobody, and `is_test` is the Rust \
         walker's: a PHP `:File` carries neither"
    );
}

#[test]
fn a_use_clause_inside_a_braced_namespace_is_invisible_to_this_producer() {
    let (nodes, _) = produce();
    assert!(
        nodes.iter().any(|n| n.id == BRACED_FILE_ID),
        "the braced-namespace file is walked and is a `:File`"
    );
    assert!(
        imports_of(&nodes, "src/Braced.php").is_empty(),
        "the import walk reads direct children of `program`, so a clause nested in a braced \
         namespace body yields no fact; this asserts the current reach rather than leaving it \
         unmeasured, and the same walk misses the classes in that body"
    );
}

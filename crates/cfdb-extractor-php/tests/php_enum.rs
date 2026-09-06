use std::path::{Path, PathBuf};

use cfdb_core::fact::{Edge, Node};
use cfdb_extractor_php::PhpProducer;
use cfdb_lang::LanguageProducer;

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/php-enum")
}

fn extract() -> (Vec<Node>, Vec<Edge>) {
    PhpProducer
        .produce(&fixture_root())
        .expect("PhpProducer.produce on the enum fixture")
}

fn item<'a>(nodes: &'a [Node], qname: &str) -> Option<&'a Node> {
    nodes.iter().find(|n| {
        n.label.as_str() == "Item" && n.props.get("qname").and_then(|v| v.as_str()) == Some(qname)
    })
}

fn prop<'a>(node: &'a Node, key: &str) -> Option<&'a str> {
    node.props.get(key).and_then(|v| v.as_str())
}

#[test]
fn an_enum_is_an_item_of_kind_enum_carrying_the_enum_declaration_construct() {
    let (nodes, _) = extract();
    let node = item(&nodes, "Enrolment\\PriorStudy").expect("the enum yields an :Item");
    assert_eq!(prop(node, "kind"), Some("enum"));
    assert_eq!(prop(node, "php_construct"), Some("enum_declaration"));
}

#[test]
fn a_class_beside_an_enum_still_carries_the_trait_kind() {
    let (nodes, _) = extract();
    let node = item(&nodes, "Enrolment\\Enrolment").expect("the class yields an :Item");
    assert_eq!(prop(node, "kind"), Some("trait"));
    assert_eq!(prop(node, "php_construct"), Some("class_declaration"));
}

#[test]
fn a_method_declared_in_an_enum_is_an_item_under_the_enum_qname() {
    let (nodes, _) = extract();
    let node =
        item(&nodes, "Enrolment\\PriorStudy::label").expect("the enum's method yields an :Item");
    assert_eq!(prop(node, "kind"), Some("fn"));
    assert_eq!(prop(node, "php_construct"), Some("method_declaration"));
}

#[test]
fn an_enum_implementing_an_in_workspace_interface_yields_one_implements_edge() {
    let (nodes, edges) = extract();
    let src = item(&nodes, "Enrolment\\PriorStudy").expect("the enum yields an :Item");
    let dst = item(&nodes, "Enrolment\\Labelled").expect("the interface yields an :Item");
    let found: Vec<&Edge> = edges
        .iter()
        .filter(|e| {
            e.label.as_str() == "IMPLEMENTS"
                && e.src.as_str() == src.id.as_str()
                && e.dst.as_str() == dst.id.as_str()
        })
        .collect();
    assert_eq!(found.len(), 1, "edges={edges:?}");
    assert_eq!(prop(src, "php_construct"), Some("enum_declaration"));
}

#[test]
fn an_enum_implementing_an_external_interface_yields_no_edge_and_no_stub() {
    let (nodes, edges) = extract();
    let src = item(&nodes, "Enrolment\\StudyDuration").expect("the enum yields an :Item");
    assert!(
        !edges
            .iter()
            .any(|e| e.label.as_str() == "IMPLEMENTS" && e.src.as_str() == src.id.as_str()),
        "edges={edges:?}"
    );
    assert!(
        item(&nodes, "Vendor\\External").is_none(),
        "the external interface must not be stubbed into the keyspace"
    );
}

#[test]
fn an_enum_case_yields_no_item_until_a_clause_admits_one() {
    let (nodes, _) = extract();
    assert!(item(&nodes, "Enrolment\\PriorStudy::None").is_none());
    assert!(
        !nodes.iter().any(|n| {
            n.label.as_str() == "Item" && n.props.get("name").and_then(|v| v.as_str()) == Some("None")
        }),
        "no :Item carries an enum case's name — cfdb-041-language-backend-trait#4.5 gates the widening"
    );
}

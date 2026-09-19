use std::fs;

use cfdb_core::fact::{Edge, Node, PropValue};
use cfdb_core::qname::supertype_node_id;
use cfdb_core::schema::EdgeLabel;
use cfdb_extractor_php::PhpProducer;
use cfdb_lang::LanguageProducer;
use tempfile::TempDir;

fn produce_php(files: &[(&str, &str)]) -> (Vec<Node>, Vec<Edge>) {
    let dir = TempDir::new().expect("tempdir");
    fs::write(
        dir.path().join("composer.json"),
        r#"{"name":"cfdb/test","type":"library","autoload":{"psr-4":{"App\\":"src/"}}}"#,
    )
    .expect("write composer.json");
    for (rel, src) in files {
        let path = dir.path().join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("mkdir -p fixture subdir");
        }
        fs::write(&path, src).expect("write php source");
    }
    PhpProducer
        .produce(dir.path())
        .expect("PhpProducer.produce")
}

fn item_id(qname: &str) -> String {
    format!("item:{qname}")
}

fn extends_edges(edges: &[Edge]) -> Vec<&Edge> {
    edges
        .iter()
        .filter(|e| e.label.as_str() == EdgeLabel::EXTENDS)
        .collect()
}

fn extends_pairs(edges: &[Edge]) -> Vec<(String, String)> {
    let mut pairs: Vec<(String, String)> = extends_edges(edges)
        .iter()
        .map(|e| (e.src.clone(), e.dst.clone()))
        .collect();
    pairs.sort();
    pairs
}

fn supertype_nodes<'a>(nodes: &'a [Node], owner_qname: &str) -> Vec<&'a Node> {
    let mut out: Vec<&Node> = nodes
        .iter()
        .filter(|n| n.id.starts_with(&format!("supertype:{owner_qname}#")))
        .collect();
    out.sort_by_key(|n| n.id.clone());
    out
}

fn node<'a>(nodes: &'a [Node], id: &str) -> Option<&'a Node> {
    nodes.iter().find(|n| n.id == id)
}

fn prop<'a>(n: &'a Node, key: &str) -> Option<&'a str> {
    n.props.get(key).and_then(PropValue::as_str)
}

fn has_supertype_targets(edges: &[Edge], owner_id: &str) -> Vec<String> {
    let mut out: Vec<String> = edges
        .iter()
        .filter(|e| e.label.as_str() == EdgeLabel::HAS_SUPERTYPE && e.src == owner_id)
        .map(|e| e.dst.clone())
        .collect();
    out.sort();
    out
}

#[test]
fn class_extending_an_in_workspace_class_emits_one_extends_edge() {
    let (nodes, edges) = produce_php(&[(
        "src/c.php",
        r#"<?php
namespace App;
class B {}
class C extends B {}
"#,
    )]);

    assert_eq!(
        extends_pairs(&edges),
        vec![(item_id(r"App\C"), item_id(r"App\B"))],
        "class C extends B (both in-workspace) must yield one EXTENDS edge C -> B",
    );
    for e in extends_edges(&edges) {
        assert_eq!(
            e.props.get("resolver").and_then(PropValue::as_str),
            Some("tree-sitter-php"),
            "every EXTENDS edge must carry resolver=tree-sitter-php; edge={e:?}",
        );
    }

    let supertypes = supertype_nodes(&nodes, r"App\C");
    assert_eq!(
        supertypes.len(),
        1,
        "one :Supertype for the one extends name"
    );
    let s = supertypes[0];
    assert_eq!(prop(s, "relation"), Some("extends"));
    assert_eq!(prop(s, "written"), Some("B"));
    assert_eq!(prop(s, "fqn"), Some(r"App\B"));
    assert_eq!(s.id, supertype_node_id(r"App\C", 0));

    assert_eq!(
        has_supertype_targets(&edges, &item_id(r"App\C")),
        vec![supertype_node_id(r"App\C", 0)],
        "HAS_SUPERTYPE from App\\C to its one :Supertype node",
    );
}

#[test]
fn class_extending_a_vendor_class_emits_no_extends_edge_but_one_supertype() {
    let (nodes, edges) = produce_php(&[(
        "src/c.php",
        r#"<?php
namespace App;
class C extends \PDO {}
"#,
    )]);

    assert!(
        extends_edges(&edges).is_empty(),
        "a vendor base class must produce no EXTENDS edge; got {:?}",
        extends_pairs(&edges),
    );
    assert!(
        node(&nodes, &item_id("PDO")).is_none(),
        "no synthetic :Item may be created for the vendor base class",
    );

    let supertypes = supertype_nodes(&nodes, r"App\C");
    assert_eq!(supertypes.len(), 1);
    let s = supertypes[0];
    assert_eq!(prop(s, "relation"), Some("extends"));
    assert_eq!(prop(s, "written"), Some(r"\PDO"));
    assert_eq!(
        prop(s, "fqn"),
        Some("PDO"),
        "fqn is resolved with no leading backslash even though the base class is external",
    );
}

#[test]
fn interface_extending_two_interfaces_one_vendor() {
    let (nodes, edges) = produce_php(&[(
        "src/i.php",
        r#"<?php
namespace App;
interface Base {}
interface I extends Base, \Countable {}
"#,
    )]);

    assert_eq!(
        extends_pairs(&edges),
        vec![(item_id(r"App\I"), item_id(r"App\Base"))],
        "only the in-workspace Base name resolves to an EXTENDS edge; Countable is vendor",
    );

    let supertypes = supertype_nodes(&nodes, r"App\I");
    assert_eq!(
        supertypes.len(),
        2,
        "both extends names get a :Supertype node, in-workspace or not"
    );
    assert_eq!(prop(supertypes[0], "relation"), Some("extends"));
    assert_eq!(prop(supertypes[0], "written"), Some("Base"));
    assert_eq!(prop(supertypes[0], "fqn"), Some(r"App\Base"));
    assert_eq!(prop(supertypes[1], "relation"), Some("extends"));
    assert_eq!(prop(supertypes[1], "written"), Some(r"\Countable"));
    assert_eq!(prop(supertypes[1], "fqn"), Some("Countable"));

    assert!(
        node(&nodes, &item_id("Countable")).is_none(),
        "no synthetic :Item for the vendor interface",
    );
}

#[test]
fn class_with_extends_and_implements_orders_extends_first() {
    let (nodes, edges) = produce_php(&[(
        "src/c.php",
        r#"<?php
namespace App;
class B {}
interface I1 {}
interface I2 {}
class C extends B implements I1, I2 {}
"#,
    )]);

    let supertypes = supertype_nodes(&nodes, r"App\C");
    assert_eq!(supertypes.len(), 3, "one extends + two implements names");

    assert_eq!(supertypes[0].id, supertype_node_id(r"App\C", 0));
    assert_eq!(prop(supertypes[0], "relation"), Some("extends"));
    assert_eq!(prop(supertypes[0], "written"), Some("B"));

    assert_eq!(supertypes[1].id, supertype_node_id(r"App\C", 1));
    assert_eq!(prop(supertypes[1], "relation"), Some("implements"));
    assert_eq!(prop(supertypes[1], "written"), Some("I1"));

    assert_eq!(supertypes[2].id, supertype_node_id(r"App\C", 2));
    assert_eq!(prop(supertypes[2], "relation"), Some("implements"));
    assert_eq!(prop(supertypes[2], "written"), Some("I2"));

    assert_eq!(
        extends_pairs(&edges),
        vec![(item_id(r"App\C"), item_id(r"App\B"))],
        "EXTENDS carries only the base_clause relation, never implements",
    );

    let implements_pairs: Vec<(String, String)> = {
        let mut pairs: Vec<(String, String)> = edges
            .iter()
            .filter(|e| e.label.as_str() == EdgeLabel::IMPLEMENTS)
            .map(|e| (e.src.clone(), e.dst.clone()))
            .collect();
        pairs.sort();
        pairs
    };
    assert_eq!(
        implements_pairs,
        vec![
            (item_id(r"App\C"), item_id(r"App\I1")),
            (item_id(r"App\C"), item_id(r"App\I2")),
        ],
        "IMPLEMENTS is unaffected by the new EXTENDS/Supertype machinery",
    );
}

#[test]
fn re_extract_is_deterministic() {
    let files = &[(
        "src/m.php",
        r#"<?php
namespace App;
class B {}
interface I {}
class C extends B implements I {}
"#,
    )];
    let run1 = produce_php(files);
    let run2 = produce_php(files);
    assert_eq!(
        format!("{run1:?}"),
        format!("{run2:?}"),
        "PhpProducer.produce must be byte-stable across re-extracts",
    );
}

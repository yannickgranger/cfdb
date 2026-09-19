use std::fs;

use cfdb_core::fact::{Edge, Node, PropValue};
use cfdb_core::schema::{EdgeLabel, Label};
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

fn node<'a>(nodes: &'a [Node], id: &str) -> Option<&'a Node> {
    nodes.iter().find(|n| n.id == id)
}

fn prop<'a>(n: &'a Node, key: &str) -> Option<&'a str> {
    n.props.get(key).and_then(PropValue::as_str)
}

fn prop_i64(n: &Node, key: &str) -> Option<i64> {
    n.props.get(key).and_then(PropValue::as_i64)
}

fn prop_bool(n: &Node, key: &str) -> Option<bool> {
    n.props.get(key).and_then(PropValue::as_bool)
}

fn params_of<'a>(nodes: &'a [Node], fn_qname: &str) -> Vec<&'a Node> {
    let mut params: Vec<&Node> = nodes
        .iter()
        .filter(|n| n.label.as_str() == Label::PARAM && prop(n, "parent_qname") == Some(fn_qname))
        .collect();
    params.sort_by_key(|n| prop_i64(n, "index").unwrap_or(i64::MAX));
    params
}

fn fields_of<'a>(nodes: &'a [Node], class_qname: &str) -> Vec<&'a Node> {
    let mut fields: Vec<&Node> = nodes
        .iter()
        .filter(|n| {
            n.label.as_str() == Label::FIELD && prop(n, "parent_qname") == Some(class_qname)
        })
        .collect();
    fields.sort_by_key(|n| prop_i64(n, "index").unwrap_or(i64::MAX));
    fields
}

fn type_of_targets(edges: &[Edge], source_id: &str) -> Vec<String> {
    let mut targets: Vec<String> = edges
        .iter()
        .filter(|e| e.label.as_str() == EdgeLabel::TYPE_OF && e.src == source_id)
        .map(|e| e.dst.clone())
        .collect();
    targets.sort();
    targets
}

fn returns_targets(edges: &[Edge], source_id: &str) -> Vec<String> {
    let mut targets: Vec<String> = edges
        .iter()
        .filter(|e| e.label.as_str() == EdgeLabel::RETURNS && e.src == source_id)
        .map(|e| e.dst.clone())
        .collect();
    targets.sort();
    targets
}

const FIXTURE: &str = r#"<?php
namespace App\C;

class Port {}
class Y {}
class A {}
class B {}

class C
{
    private int|float $a, $b = 2;

    public function __construct(private readonly ?Port $port)
    {
    }

    public function m(self $s, \Vendor\X&Y $i, (A&B)|null $d, string ...$rest): static
    {
    }
}
"#;

#[test]
fn param_set_carries_index_name_type_path_and_normalized_arms() {
    let (nodes, _edges) = produce_php(&[("src/c.php", FIXTURE)]);
    let params = params_of(&nodes, r"App\C\C::m");
    assert_eq!(
        params.len(),
        4,
        "m() declares 4 formal parameters (self, intersection, DNF, variadic); got {params:?}"
    );

    let s = params[0];
    assert_eq!(prop_i64(s, "index"), Some(0));
    assert_eq!(prop(s, "name"), Some("s"));
    assert_eq!(prop(s, "type_path"), Some("self"));
    assert_eq!(prop(s, "type_normalized"), Some(r"App\C\C"));
    assert_eq!(prop_bool(s, "is_self"), Some(false));

    let i = params[1];
    assert_eq!(prop(i, "name"), Some("i"));
    assert_eq!(prop(i, "type_path"), Some(r"\Vendor\X&Y"));
    assert_eq!(prop(i, "type_normalized"), Some(r"(Vendor\X&App\C\Y)"));

    let d = params[2];
    assert_eq!(prop(d, "name"), Some("d"));
    assert_eq!(prop(d, "type_path"), Some("(A&B)|null"));
    assert_eq!(prop(d, "type_normalized"), Some(r"(App\C\A&App\C\B)|null"));

    let rest = params[3];
    assert_eq!(prop(rest, "name"), Some("rest"));
    assert_eq!(prop(rest, "type_path"), Some("string"));
    assert_eq!(prop(rest, "type_normalized"), Some("string"));
}

#[test]
fn the_promoted_parameter_is_both_a_param_and_a_field() {
    let (nodes, _edges) = produce_php(&[("src/c.php", FIXTURE)]);

    let ctor_params = params_of(&nodes, r"App\C\C::__construct");
    assert_eq!(
        ctor_params.len(),
        1,
        "one promoted parameter on __construct"
    );
    let port_param = ctor_params[0];
    assert_eq!(prop(port_param, "name"), Some("port"));
    assert_eq!(prop(port_param, "type_path"), Some("?Port"));
    assert_eq!(
        prop(port_param, "type_normalized"),
        Some(r"App\C\Port|null")
    );
    assert_eq!(prop_bool(port_param, "is_self"), Some(false));

    let fields = fields_of(&nodes, r"App\C\C");
    let port_field = fields
        .iter()
        .find(|f| prop(f, "name") == Some("port"))
        .expect("promoted parameter also emits a :Field owned by the class");
    assert_eq!(prop(port_field, "type_path"), Some("?Port"));
    assert_eq!(
        prop(port_field, "type_normalized"),
        Some(r"App\C\Port|null")
    );
    assert_eq!(
        prop_i64(port_field, "index"),
        Some(2),
        "the promoted field is numbered after the two body properties $a, $b"
    );
}

#[test]
fn two_fields_for_a_b_share_the_one_declared_type() {
    let (nodes, _edges) = produce_php(&[("src/c.php", FIXTURE)]);
    let fields = fields_of(&nodes, r"App\C\C");
    let a = fields
        .iter()
        .find(|f| prop(f, "name") == Some("a"))
        .expect("field a present");
    let b = fields
        .iter()
        .find(|f| prop(f, "name") == Some("b"))
        .expect("field b present");

    assert_eq!(prop_i64(a, "index"), Some(0));
    assert_eq!(prop_i64(b, "index"), Some(1));
    assert_eq!(prop(a, "type_path"), Some("int|float"));
    assert_eq!(prop(b, "type_path"), Some("int|float"));
    assert_eq!(prop(a, "type_normalized"), Some("int|float"));
    assert_eq!(prop(b, "type_normalized"), Some("int|float"));
}

#[test]
fn return_type_normalized_resolves_static_to_the_enclosing_class() {
    let (nodes, _edges) = produce_php(&[("src/c.php", FIXTURE)]);
    let m = node(&nodes, &item_id(r"App\C\C::m")).expect(r"App\C\C::m :Item present");
    assert_eq!(prop(m, "return_type_path"), Some("static"));
    assert_eq!(prop(m, "return_type_normalized"), Some(r"App\C\C"));
}

#[test]
fn one_type_of_edge_per_in_workspace_arm_and_none_for_vendor_or_float() {
    let (nodes, edges) = produce_php(&[("src/c.php", FIXTURE)]);

    let s_id = format!("param:{}#0", r"App\C\C::m");
    assert_eq!(type_of_targets(&edges, &s_id), vec![item_id(r"App\C\C")]);

    let i_id = format!("param:{}#1", r"App\C\C::m");
    assert_eq!(
        type_of_targets(&edges, &i_id),
        vec![item_id(r"App\C\Y")],
        "the Vendor\\X member of the intersection is out of workspace and gets no edge"
    );

    let d_id = format!("param:{}#2", r"App\C\C::m");
    assert_eq!(
        type_of_targets(&edges, &d_id),
        vec![item_id(r"App\C\A"), item_id(r"App\C\B")],
    );

    let rest_id = format!("param:{}#3", r"App\C\C::m");
    assert!(
        type_of_targets(&edges, &rest_id).is_empty(),
        "a primitive-only arm (string) never yields TYPE_OF"
    );

    let port_field_id = format!("field:{}.port", r"App\C\C");
    assert_eq!(
        type_of_targets(&edges, &port_field_id),
        vec![item_id(r"App\C\Port")],
    );

    let m_id = item_id(r"App\C\C::m");
    assert_eq!(returns_targets(&edges, &m_id), vec![item_id(r"App\C\C")]);

    assert!(
        node(&nodes, &item_id(r"Vendor\X")).is_none(),
        "the PHP producer is closed-world: an out-of-workspace type arm never mints an :Item"
    );
}

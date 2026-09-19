use std::fs;

use cfdb_core::fact::{Edge, Node, PropValue};
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

const FIXTURE: &str = r#"<?php
namespace App\Cx;

const K = 1;

class D
{
    const URL = 'https://x';

    private string $dsn = 'redis://' . $h;

    public function __construct(private readonly int $retries = 3)
    {
    }

    public function m(int $a, int $b = 7)
    {
    }
}
"#;

#[test]
fn top_level_const_is_an_item_with_kind_const_and_value_text() {
    let (nodes, _edges) = produce_php(&[("src/d.php", FIXTURE)]);
    let k = node(&nodes, &item_id(r"App\Cx\K")).expect(r"App\Cx\K :Item present");
    assert_eq!(prop(k, "kind"), Some("const"));
    assert_eq!(prop(k, "name"), Some("K"));
    assert_eq!(prop(k, "qname"), Some(r"App\Cx\K"));
    assert_eq!(prop(k, "php_construct"), Some("const_declaration"));
    assert_eq!(prop(k, "value_text"), Some("1"));
}

#[test]
fn class_const_qname_is_class_qname_double_colon_name_value_text_byte_exact_with_quotes() {
    let (nodes, _edges) = produce_php(&[("src/d.php", FIXTURE)]);
    let url = node(&nodes, &item_id(r"App\Cx\D::URL")).expect(r"App\Cx\D::URL :Item present");
    assert_eq!(prop(url, "kind"), Some("const"));
    assert_eq!(prop(url, "name"), Some("URL"));
    assert_eq!(
        prop(url, "value_text"),
        Some("'https://x'"),
        "value_text is the verbatim bytes of the value expression, quotes included"
    );
}

#[test]
fn property_default_is_byte_exact_including_a_concatenation_expression() {
    let (nodes, _edges) = produce_php(&[("src/d.php", FIXTURE)]);
    let dsn_id = format!("field:{}.dsn", r"App\Cx\D");
    let dsn = node(&nodes, &dsn_id).expect("dsn :Field present");
    assert_eq!(
        prop(dsn, "default_text"),
        Some("'redis://' . $h"),
        "default_text is byte-faithful, including the concatenation and the quotes"
    );
}

#[test]
fn promoted_parameter_default_lands_on_both_the_param_and_the_field() {
    let (nodes, _edges) = produce_php(&[("src/d.php", FIXTURE)]);
    let retries_param_id = format!("param:{}#0", r"App\Cx\D::__construct");
    let retries_param = node(&nodes, &retries_param_id).expect("retries :Param present");
    assert_eq!(prop(retries_param, "default_text"), Some("3"));

    let retries_field_id = format!("field:{}.retries", r"App\Cx\D");
    let retries_field = node(&nodes, &retries_field_id).expect("retries :Field present");
    assert_eq!(prop(retries_field, "default_text"), Some("3"));
}

#[test]
fn a_parameter_with_no_default_carries_no_default_text_property() {
    let (nodes, _edges) = produce_php(&[("src/d.php", FIXTURE)]);
    let a_id = format!("param:{}#0", r"App\Cx\D::m");
    let a = node(&nodes, &a_id).expect("a :Param present");
    assert_eq!(
        prop(a, "default_text"),
        None,
        "a parameter with no default_value field must not carry the default_text attribute at all"
    );

    let b_id = format!("param:{}#1", r"App\Cx\D::m");
    let b = node(&nodes, &b_id).expect("b :Param present");
    assert_eq!(prop(b, "default_text"), Some("7"));
}

use std::fs;

use cfdb_core::fact::{Edge, Node, PropValue};
use cfdb_core::schema::{EdgeLabel, Label};
use cfdb_extractor_php::PhpProducer;
use cfdb_lang::LanguageProducer;
use tempfile::TempDir;

const COMPOSER: &str = r#"{
  "name": "cfdb/test",
  "autoload": { "psr-4": { "App\\": "src/" } }
}"#;

const SOURCE: &str = r#"<?php

namespace App;

use Symfony\Component\DependencyInjection\Attribute\Autowire;

#[A, B]
class Widget
{
    #[Route('/widgets')]
    public function handle(
        #[Autowire('%app.dsn%'), Deprecated] private readonly string $dsn,
    ): void {
    }

    public function plain(): void
    {
    }

    #[Deprecated]
    public string $label;
}
"#;

fn produce() -> (Vec<Node>, Vec<Edge>) {
    let dir = TempDir::new().expect("tempdir");
    fs::write(dir.path().join("composer.json"), COMPOSER).expect("write composer.json");
    fs::create_dir_all(dir.path().join("src")).expect("mkdir -p src");
    fs::write(dir.path().join("src/Widget.php"), SOURCE).expect("write Widget.php");
    PhpProducer
        .produce(dir.path())
        .expect("PhpProducer.produce")
}

fn prop<'a>(n: &'a Node, key: &str) -> Option<&'a str> {
    n.props.get(key).and_then(PropValue::as_str)
}

fn attributes_owned_by<'a>(nodes: &'a [Node], edges: &[Edge], owner_id: &str) -> Vec<&'a Node> {
    let mut owned: Vec<&Node> = edges
        .iter()
        .filter(|e| e.label.as_str() == EdgeLabel::HAS_ATTRIBUTE && e.src == owner_id)
        .filter_map(|e| nodes.iter().find(|n| n.id == e.dst))
        .collect();
    owned.sort_by_key(|n| n.id.clone());
    owned
}

#[test]
fn a_grouped_attribute_on_a_class_is_two_attribute_nodes_idx_zero_and_one() {
    let (nodes, edges) = produce();
    let mut owned = attributes_owned_by(&nodes, &edges, "item:App\\Widget");
    owned.sort_by_key(|n| n.id.clone());
    assert_eq!(
        owned.len(),
        2,
        "`#[A, B]` is a grouped attribute list: two :Attribute nodes, not one joined string: {owned:?}"
    );
    let ids: Vec<&str> = owned.iter().map(|n| n.id.as_str()).collect();
    assert_eq!(
        ids,
        vec!["attr:item:App\\Widget#0", "attr:item:App\\Widget#1"],
        "idx is the zero-based order on the owner, `attr:{{owner id}}#{{idx}}`"
    );
    let written: Vec<Option<&str>> = owned.iter().map(|n| prop(n, "written")).collect();
    assert_eq!(
        written,
        vec![Some("A"), Some("B")],
        "source order: A before B"
    );
    assert_eq!(
        prop(owned[0], "fqn"),
        Some("App\\A"),
        "`A` carries no `use` import, so it resolves within the current namespace"
    );
    assert_eq!(prop(owned[1], "fqn"), Some("App\\B"));
}

#[test]
fn an_attribute_on_a_method_is_owned_by_the_method_item() {
    let (nodes, edges) = produce();
    let owned = attributes_owned_by(&nodes, &edges, "item:App\\Widget::handle");
    assert_eq!(owned.len(), 1, "`#[Route(...)]` on `handle`: {owned:?}");
    assert_eq!(prop(owned[0], "written"), Some("Route"));
    assert_eq!(
        prop(owned[0], "fqn"),
        Some("App\\Route"),
        "`Route` carries no `use` import, so it resolves within the current namespace, the same \
         rule `:Import.fqn` follows"
    );
}

#[test]
fn a_method_with_no_attributes_owns_none() {
    let (nodes, edges) = produce();
    let owned = attributes_owned_by(&nodes, &edges, "item:App\\Widget::plain");
    assert!(
        owned.is_empty(),
        "`plain()` declares no attribute: {owned:?}"
    );
}

#[test]
fn a_promoted_parameter_attribute_is_two_attribute_nodes_one_per_owner() {
    let (nodes, edges) = produce();
    let param_owned = attributes_owned_by(&nodes, &edges, "param:App\\Widget::handle#0");
    let field_owned = attributes_owned_by(&nodes, &edges, "field:App\\Widget.dsn");

    assert_eq!(
        param_owned.len(),
        2,
        "the :Param owns its own :Attribute copy of both grouped attributes: {param_owned:?}"
    );
    assert_eq!(
        field_owned.len(),
        2,
        "the :Field owns its own :Attribute copy of both grouped attributes: {field_owned:?}"
    );
    for (p, f) in param_owned.iter().zip(field_owned.iter()) {
        assert_ne!(
            p.id, f.id,
            "cfdb-062-php-declared-shapes#3.5: a promoted parameter's attribute is TWO \
             :Attribute nodes, one owned by the :Param and one by the :Field, each its own id"
        );
    }
    assert_eq!(prop(param_owned[0], "written"), Some("Autowire"));
    assert_eq!(
        prop(param_owned[0], "fqn"),
        Some("Symfony\\Component\\DependencyInjection\\Attribute\\Autowire"),
        "fqn resolves through the same ImportTable::resolve as :Import.fqn"
    );
}

#[test]
fn a_promoted_parameter_with_two_attributes_has_matching_idx_between_param_and_field_owners() {
    let (nodes, edges) = produce();
    let param_owned = attributes_owned_by(&nodes, &edges, "param:App\\Widget::handle#0");
    let field_owned = attributes_owned_by(&nodes, &edges, "field:App\\Widget.dsn");

    assert_eq!(
        param_owned.len(),
        2,
        "grouped `#[Autowire(...), Deprecated]`: {param_owned:?}"
    );
    assert_eq!(
        field_owned.len(),
        2,
        "grouped `#[Autowire(...), Deprecated]`: {field_owned:?}"
    );

    fn idx_of(id: &str) -> &str {
        id.rsplit('#').next().expect("attr id has a # suffix")
    }

    for i in 0..2 {
        assert_eq!(
            idx_of(&param_owned[i].id),
            idx_of(&field_owned[i].id),
            "position {i}: :Param id {:?} and :Field id {:?} must carry the same idx suffix \
             since both read idx fresh off the identical property_promotion_parameter node's \
             own attribute_list, independent of which owner is calling",
            param_owned[i].id,
            field_owned[i].id
        );
        assert_eq!(
            prop(param_owned[i], "written"),
            prop(field_owned[i], "written"),
            "position {i}: the :Param-owned and :Field-owned :Attribute at the same idx must \
             name the same attribute, or the two owners' copies have desynced"
        );
        assert_eq!(
            prop(param_owned[i], "fqn"),
            prop(field_owned[i], "fqn"),
            "position {i}: fqn must also match — same source attribute, two owners"
        );
    }
    assert_eq!(
        prop(param_owned[0], "written"),
        Some("Autowire"),
        "source order: Autowire first"
    );
    assert_eq!(
        prop(param_owned[1], "written"),
        Some("Deprecated"),
        "source order: Deprecated second"
    );
}

#[test]
fn an_attribute_on_a_plain_property_is_owned_by_its_field() {
    let (nodes, edges) = produce();
    let owned = attributes_owned_by(&nodes, &edges, "field:App\\Widget.label");
    assert_eq!(owned.len(), 1, "`#[Deprecated]` on `$label`: {owned:?}");
    assert_eq!(prop(owned[0], "written"), Some("Deprecated"));
    assert_eq!(prop(owned[0], "fqn"), Some("App\\Deprecated"));
}

#[test]
fn every_attribute_node_carries_file_and_line() {
    let (nodes, _edges) = produce();
    let attrs: Vec<&Node> = nodes
        .iter()
        .filter(|n| n.label.as_str() == Label::ATTRIBUTE)
        .collect();
    assert_eq!(
        attrs.len(),
        8,
        "2 (class A, B) + 1 (Route) + 2 (Autowire+Deprecated on :Param) + 2 \
         (Autowire+Deprecated on :Field) + 1 (Deprecated on $label) = 8 total: {attrs:?}"
    );
    for n in &attrs {
        assert_eq!(prop(n, "file"), Some("src/Widget.php"));
        assert!(n.props.contains_key("line"), "line recorded: {n:?}");
    }
}

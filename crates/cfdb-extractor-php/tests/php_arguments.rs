use std::collections::BTreeMap;
use std::fs;

use cfdb_core::fact::{Node, PropValue};
use cfdb_core::schema::ARG_KINDS;
use cfdb_extractor_php::PhpProducer;
use cfdb_lang::LanguageProducer;
use tempfile::TempDir;

type Argument = (u32, String, String);
type ArgumentsByCall = BTreeMap<String, Vec<Argument>>;

const COMPOSER: &str = r#"{
  "name": "cfdb/test",
  "autoload": { "psr-4": { "App\\": "src/" } }
}"#;

const SOURCE: &str = r#"<?php

namespace App;

class Fixture
{
    public function run(): void
    {
        new \DateTimeImmutable('now');
        new \DateTimeImmutable('2024-01-01');
        new \DateTimeImmutable();
        $this->send($msg, 2);
        f(name: $x);
        f(label: 'text');
        f(...$xs);
        f(&$x);
        f(...);
        f($after);
        g("a $b");
        g(Foo::class);
        g(1.5);
        g(true);
        g(null);
        g($obj->field);
        g($obj->call());
        g(other());
        g(new Thing());
        g([1, 2]);
    }
}
"#;

fn produce() -> (Vec<Node>, ArgumentsByCall) {
    let dir = TempDir::new().expect("tempdir");
    fs::write(dir.path().join("composer.json"), COMPOSER).expect("write composer.json");
    fs::create_dir_all(dir.path().join("src")).expect("mkdir -p src");
    fs::write(dir.path().join("src/Fixture.php"), SOURCE).expect("write Fixture.php");
    let (nodes, edges) = PhpProducer
        .produce(dir.path())
        .expect("PhpProducer.produce");

    let args: BTreeMap<&str, &Node> = nodes
        .iter()
        .filter(|n| n.label.as_str() == "Argument")
        .map(|n| (n.id.as_str(), n))
        .collect();

    let mut by_call: ArgumentsByCall = BTreeMap::new();
    for edge in edges.iter().filter(|e| e.label.as_str() == "HAS_ARG") {
        let node = args
            .get(edge.dst.as_str())
            .expect("HAS_ARG names an :Argument");
        let position = match node.props.get("position") {
            Some(PropValue::Int(p)) => u32::try_from(*p).expect("position fits u32"),
            other => panic!("`:Argument.position` must be an Int, got {other:?}"),
        };
        assert_eq!(
            node.id,
            format!("arg:{}#{position}", edge.src),
            "the node id is `arg:{{callsite_id}}#{{position}}`"
        );
        by_call.entry(edge.src.clone()).or_default().push((
            position,
            node.props
                .get("kind")
                .and_then(PropValue::as_str)
                .expect("kind")
                .to_string(),
            node.props
                .get("source_text")
                .and_then(PropValue::as_str)
                .expect("source_text")
                .to_string(),
        ));
    }
    for list in by_call.values_mut() {
        list.sort();
    }
    (nodes, by_call)
}

fn of(
    by_call: &BTreeMap<String, Vec<(u32, String, String)>>,
    callee: &str,
) -> Vec<(u32, String, String)> {
    let key = by_call
        .keys()
        .find(|k| k.contains(&format!(":{callee}:")))
        .unwrap_or_else(|| panic!("a call site for `{callee}`: {:?}", by_call.keys()));
    by_call[key].clone()
}

fn all_of<'m>(by_call: &'m ArgumentsByCall, callee: &str) -> Vec<&'m Vec<Argument>> {
    let needle = format!(":{callee}:");
    by_call
        .iter()
        .filter(|(k, _)| k.contains(&needle))
        .map(|(_, v)| v)
        .collect()
}

fn kinds_of(by_call: &ArgumentsByCall, callee: &str) -> Vec<String> {
    all_of(by_call, callee)
        .into_iter()
        .flatten()
        .map(|(_, k, _)| k.clone())
        .collect()
}

#[test]
fn an_argument_carries_its_position_kind_and_verbatim_source_text() {
    let (nodes, by_call) = produce();

    let constructions = nodes
        .iter()
        .filter(|n| n.label.as_str() == "CallSite")
        .filter(|n| n.props.get("kind").and_then(PropValue::as_str) == Some("new"))
        .count();
    assert_eq!(
        constructions, 4,
        "three `\\DateTimeImmutable` constructions and one `new Thing()`; the argument-less \
         one still emits its call site"
    );

    let flat: Vec<Argument> = all_of(&by_call, "\\DateTimeImmutable")
        .into_iter()
        .flatten()
        .cloned()
        .collect();
    assert_eq!(
        flat,
        vec![
            (0, "literal".to_string(), "'now'".to_string()),
            (0, "literal".to_string(), "'2024-01-01'".to_string()),
        ],
        "the clock read and its discriminating sibling carry byte-exact source text including \
         the quotes, and the third construction contributes no argument at all"
    );
}

#[test]
fn position_zero_is_the_receiver_on_a_member_call() {
    let (_, by_call) = produce();
    assert_eq!(
        of(&by_call, "send"),
        vec![
            (0, "path".to_string(), "$this".to_string()),
            (1, "path".to_string(), "$msg".to_string()),
            (2, "literal".to_string(), "2".to_string()),
        ],
        "RECEIVER_POSITION means the same thing in every producer: the receiver is 0 and the \
         first written argument is 1"
    );
}

#[test]
fn a_named_a_spread_and_a_by_reference_argument_are_read_at_the_wrapper() {
    let (_, by_call) = produce();
    let f = by_call
        .iter()
        .filter(|(k, _)| k.contains(":f:"))
        .map(|(_, v)| v.clone())
        .collect::<Vec<_>>();

    let flat: Vec<Argument> = f.iter().flatten().cloned().collect();
    assert_eq!(
        flat,
        vec![
            (0, "path".to_string(), "name: $x".to_string()),
            (0, "literal".to_string(), "label: 'text'".to_string()),
            (0, "path".to_string(), "...$xs".to_string()),
            (0, "ref".to_string(), "&$x".to_string()),
            (0, "path".to_string(), "$after".to_string()),
        ],
        "source_text is the WRAPPER's range, so it carries `name:`, `...` and `&`; the kind \
         comes from the value and not the label — `label: 'text'` is `literal`, which a \
         classifier reading the wrapper's first named child would call `path` — from the \
         unwrapped child of a spread, and \
         from the reference_modifier field for a by-reference argument. `f(...)` is a \
         first-class callable and emits no argument at all, so the sibling `f($after)` keeps \
         position 0"
    );
}

#[test]
fn every_row_of_the_kind_table_maps_and_none_falls_outside_the_closed_set() {
    let (nodes, by_call) = produce();
    assert_eq!(
        kinds_of(&by_call, "g"),
        vec![
            "literal",
            "path",
            "literal",
            "literal",
            "literal",
            "path",
            "method_call",
            "call",
            "call",
            "other",
        ],
        "an encapsed string, a class constant, a float, a boolean, null, a property access, \
         a method call, a free call, a construction and an array, in source order"
    );

    for node in nodes.iter().filter(|n| n.label.as_str() == "Argument") {
        let kind = node
            .props
            .get("kind")
            .and_then(PropValue::as_str)
            .expect("kind");
        assert!(
            ARG_KINDS.contains(&kind),
            "every emitted kind is one of the closed set `cfdb_core::schema::ARG_KINDS`: {kind}"
        );
    }
}

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

class Fixture
{
    public function reads(): void
    {
        $a = $_ENV['A'];
        $b = $_SERVER['B'] ?? 'x';
    }

    public function writes(): void
    {
        $_SESSION['k'] = 1;
    }

    public function repeatedReads(): void
    {
        $x = $_ENV['X'];
        $y = $_ENV['Y'];
    }

    public function notSuperglobal(): void
    {
        $env = getenv('PATH');
    }
}
"#;

fn produce() -> (Vec<Node>, Vec<Edge>) {
    let dir = TempDir::new().expect("tempdir");
    fs::write(dir.path().join("composer.json"), COMPOSER).expect("write composer.json");
    fs::create_dir_all(dir.path().join("src")).expect("mkdir -p src");
    fs::write(dir.path().join("src/Fixture.php"), SOURCE).expect("write Fixture.php");
    PhpProducer
        .produce(dir.path())
        .expect("PhpProducer.produce")
}

fn global_reads(nodes: &[Node]) -> Vec<&Node> {
    nodes
        .iter()
        .filter(|n| n.label.as_str() == Label::GLOBAL_READ)
        .collect()
}

fn prop<'a>(n: &'a Node, key: &str) -> Option<&'a str> {
    n.props.get(key).and_then(PropValue::as_str)
}

fn reads_named<'a>(nodes: &'a [Node], caller_qname: &str) -> Vec<&'a Node> {
    let mut matches: Vec<&Node> = global_reads(nodes)
        .into_iter()
        .filter(|n| prop(n, "caller_qname") == Some(caller_qname))
        .collect();
    matches.sort_by_key(|n| n.id.clone());
    matches
}

#[test]
fn a_subscript_read_of_a_superglobal_is_one_global_read() {
    let (nodes, _) = produce();
    let reads = reads_named(&nodes, r"App\Fixture::reads");
    assert_eq!(
        reads.len(),
        2,
        "`$_ENV['A']` and `$_SERVER['B'] ?? 'x'` are two distinct superglobal reads: {reads:?}"
    );
    let names: Vec<&str> = reads.iter().filter_map(|n| prop(n, "name")).collect();
    assert_eq!(
        names,
        vec!["_ENV", "_SERVER"],
        "name carries the superglobal WITHOUT its leading `$`"
    );
    for n in &reads {
        assert_eq!(prop(n, "caller_qname"), Some(r"App\Fixture::reads"));
        assert!(n.props.contains_key("line"), "line is recorded: {n:?}");
    }
}

#[test]
fn a_write_to_a_superglobal_counts_exactly_like_a_read() {
    let (nodes, _) = produce();
    let writes = reads_named(&nodes, r"App\Fixture::writes");
    assert_eq!(
        writes.len(),
        1,
        "`$_SESSION['k'] = 1` is a write, and a write counts (RFC: a superglobal written \
         outside the wiring is configuration held by a class all the same): {writes:?}"
    );
    assert_eq!(prop(writes[0], "name"), Some("_SESSION"));
}

#[test]
fn two_reads_of_one_name_in_one_method_get_ordinals_zero_and_one() {
    let (nodes, _) = produce();
    let reads = reads_named(&nodes, r"App\Fixture::repeatedReads");
    assert_eq!(reads.len(), 2, "`$_ENV['X']` and `$_ENV['Y']`: {reads:?}");
    let mut ids: Vec<&str> = reads.iter().map(|n| n.id.as_str()).collect();
    ids.sort();
    assert_eq!(
        ids,
        vec![
            r"globalread:App\Fixture::repeatedReads:_ENV:0",
            r"globalread:App\Fixture::repeatedReads:_ENV:1",
        ],
        "the node id is `globalread:{{caller_qname}}:{{name}}:{{idx}}`, idx the zero-based \
         ordinal among reads of that same name in that same caller"
    );
}

#[test]
fn an_ordinary_variable_is_not_a_global_read() {
    let (nodes, _) = produce();
    let none = reads_named(&nodes, r"App\Fixture::notSuperglobal");
    assert!(
        none.is_empty(),
        "`$env` is lowercase and not one of the nine superglobals, and `getenv('PATH')`'s \
         string literal argument is not a variable_name at all: {none:?}"
    );
}

#[test]
fn every_global_read_is_reachable_from_its_caller_item_via_reads_global() {
    let (nodes, edges) = produce();
    let reads = global_reads(&nodes);
    let reads_global: Vec<&Edge> = edges
        .iter()
        .filter(|e| e.label.as_str() == EdgeLabel::READS_GLOBAL)
        .collect();
    assert_eq!(
        reads_global.len(),
        reads.len(),
        "every :GlobalRead is reached by exactly one READS_GLOBAL edge from its caller Item"
    );
    for read in &reads {
        let caller_qname = prop(read, "caller_qname").expect("caller_qname");
        let expected_src = format!("item:{caller_qname}");
        assert!(
            reads_global
                .iter()
                .any(|e| e.src == expected_src && e.dst == read.id),
            "READS_GLOBAL runs Item -> GlobalRead (mirrors INVOKES_AT / MATCHES_AT), not the \
             other way around: no edge {expected_src} -> {} among {reads_global:?}",
            read.id
        );
    }
}

#[test]
fn a_global_read_is_never_a_call_site() {
    let (nodes, _) = produce();
    let call_sites: Vec<&Node> = nodes
        .iter()
        .filter(|n| n.label.as_str() == Label::CALL_SITE)
        .collect();
    assert!(
        call_sites
            .iter()
            .all(|cs| prop(cs, "callee_path") != Some("_ENV")
                && prop(cs, "callee_path") != Some("_SERVER")
                && prop(cs, "callee_path") != Some("_SESSION")),
        "a superglobal access has no callee and must never surface as a :CallSite: {call_sites:?}"
    );
}

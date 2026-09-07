use std::collections::{BTreeMap, BTreeSet};
use std::fs;

use cfdb_core::fact::Node;
use cfdb_core::schema::schema_describe;
use cfdb_extractor_php::PhpProducer;
use cfdb_lang::LanguageProducer;
use tempfile::TempDir;

const COMPOSER: &str = r#"{
  "name": "cfdb/test",
  "autoload": { "psr-4": { "App\\": "src/" } },
  "autoload-dev": { "psr-4": { "App\\Tests\\": "tests/" } }
}"#;

const SOURCE: &str = r#"<?php

namespace App;

use App\Other\Contract;
use App\Other\Contract as Aliased;

interface Port
{
}

enum Status: string
{
    case Open = 'open';
}

class Service implements Port
{
    public function run(): void
    {
        Contract::make();
        $this->run();
    }
}

function helper(): void
{
    Service::class;
}
"#;

const TEST_SOURCE: &str = r#"<?php

namespace App\Tests;

class ServiceTest
{
    public function testRun(): void
    {
        \strlen('x');
    }
}
"#;

fn produce() -> Vec<Node> {
    let dir = TempDir::new().expect("tempdir");
    fs::write(dir.path().join("composer.json"), COMPOSER).expect("write composer.json");
    fs::create_dir_all(dir.path().join("src")).expect("mkdir -p src");
    fs::create_dir_all(dir.path().join("tests")).expect("mkdir -p tests");
    fs::write(dir.path().join("src/Service.php"), SOURCE).expect("write Service.php");
    fs::write(dir.path().join("tests/ServiceTest.php"), TEST_SOURCE)
        .expect("write ServiceTest.php");
    let (nodes, _) = PhpProducer
        .produce(dir.path())
        .expect("PhpProducer.produce");
    nodes
}

fn undeclared(nodes: &[Node]) -> BTreeMap<String, BTreeSet<String>> {
    let declared: BTreeMap<String, BTreeSet<String>> = schema_describe()
        .nodes
        .into_iter()
        .map(|n| {
            (
                n.label.as_str().to_string(),
                n.attributes.into_iter().map(|a| a.name).collect(),
            )
        })
        .collect();
    let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for node in nodes {
        let label = node.label.as_str();
        let known = declared.get(label);
        for key in node.props.keys() {
            if !known.is_some_and(|k| k.contains(key)) {
                out.entry(label.to_string())
                    .or_default()
                    .insert(key.clone());
            }
        }
    }
    out
}

#[test]
fn every_attribute_the_php_producer_writes_is_declared() {
    let nodes = produce();
    let labels: BTreeSet<&str> = nodes.iter().map(|n| n.label.as_str()).collect();
    assert_eq!(
        labels,
        BTreeSet::from(["CallSite", "Crate", "File", "Import", "Item", "Module"]),
        "the fixture must reach every label this producer emits, or the assertion holds over a subset"
    );
    assert_eq!(
        undeclared(&nodes),
        BTreeMap::new(),
        "an attribute a producer writes but `schema_describe` does not declare is load-bearing and deniable at once: a rule can fence on it while `cfdb schema-describe` denies it exists (issue #693). Declare it in the descriptor rather than removing the emission."
    );
}

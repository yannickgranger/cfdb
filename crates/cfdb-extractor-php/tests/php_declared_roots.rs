use std::fs;

use cfdb_core::fact::{Node, PropValue};
use cfdb_extractor_php::PhpProducer;
use cfdb_lang::LanguageProducer;
use tempfile::TempDir;

const COMPOSER: &str = r#"{
  "name": "cfdb/test",
  "autoload": { "psr-4": { "App\\": "src/" } },
  "autoload-dev": { "psr-4": { "App\\Tests\\": "tests/" } }
}"#;

const GENERATED_CONTAINER: &str = "<?php

namespace ContainerXyz;

class App_KernelDevDebugContainer
{
    public function getService(): void
    {
        \\strlen('generated');
    }
}
";

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

fn files_of(nodes: &[Node]) -> Vec<&str> {
    let mut files: Vec<&str> = nodes
        .iter()
        .filter_map(|n| n.props.get("file").and_then(PropValue::as_str))
        .collect();
    files.sort_unstable();
    files.dedup();
    files
}

fn source(body: &str) -> String {
    format!("<?php\n\nnamespace App;\n\nclass Thing\n{{\n    public function run(): void\n    {{\n{body}\n    }}\n}}\n")
}

#[test]
fn generated_code_outside_every_declared_root_yields_no_facts() {
    let nodes = produce(
        COMPOSER,
        &[
            ("src/Service.php", &source("        strlen('a');")),
            ("tests/ServiceTest.php", &source("        strlen('a');")),
            (
                "var/cache/ContainerXyz/App_KernelDevDebugContainer.php",
                GENERATED_CONTAINER,
            ),
        ],
    );
    assert_eq!(
        files_of(&nodes),
        vec!["src/Service.php", "tests/ServiceTest.php"],
        "only the roots composer declares are walked; a built project's var/ is machine-written and is not the project's source"
    );
}

#[test]
fn a_workspace_declaring_no_root_walks_nothing() {
    let nodes = produce(
        r#"{"name":"cfdb/test","type":"library"}"#,
        &[("src/Service.php", &source("        strlen('a');"))],
    );
    assert!(
        files_of(&nodes).is_empty(),
        "a manifest declaring no autoload root declares no source; walking the whole tree instead is the convention this producer does not invent"
    );
}

#[test]
fn a_file_named_directly_by_an_autoload_files_entry_is_walked() {
    let nodes = produce(
        r#"{"name":"cfdb/test","autoload":{"files":["bootstrap/helpers.php"]}}"#,
        &[("bootstrap/helpers.php", &source("        strlen('a');"))],
    );
    assert_eq!(
        files_of(&nodes),
        vec!["bootstrap/helpers.php"],
        "an autoload `files` entry names one file, not a directory"
    );
}

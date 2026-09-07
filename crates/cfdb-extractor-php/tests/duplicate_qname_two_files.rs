use std::path::Path;

use cfdb_core::fact::{Node, PropValue};
use cfdb_core::result::WarningKind;
use cfdb_core::schema::{Keyspace, Label};
use cfdb_core::store::StoreBackend;
use cfdb_extractor_php::PhpProducer;
use cfdb_lang::LanguageProducer;
use cfdb_petgraph::PetgraphStore;

const FIXTURE: &str = "tests/fixtures/php-same-name-two-files";
const CONTENDED_ID: &str = "item:App\\Thing";

fn produced_nodes() -> Vec<Node> {
    let (nodes, _) = PhpProducer
        .produce(Path::new(FIXTURE))
        .expect("produce on the fixture must succeed");
    assert!(
        !nodes.is_empty(),
        "fixture yielded no nodes, so this test asserted nothing"
    );
    nodes
}

fn file_of(node: &Node) -> &str {
    match node.props.get("file") {
        Some(PropValue::Str(f)) => f.as_str(),
        other => panic!("node {} carries no string `file` prop: {other:?}", node.id),
    }
}

#[test]
fn one_qname_declared_in_two_files_yields_two_items() {
    let nodes = produced_nodes();
    let contended: Vec<&Node> = nodes
        .iter()
        .filter(|n| n.label.as_str() == Label::ITEM && n.id == CONTENDED_ID)
        .collect();
    assert_eq!(
        contended.len(),
        2,
        "both declarations of {CONTENDED_ID} must reach the store, got {} node(s)",
        contended.len()
    );
    let mut files: Vec<&str> = contended.iter().map(|n| file_of(n)).collect();
    files.sort_unstable();
    assert_eq!(
        files,
        vec!["src/legacy/Thing.php", "src/modern/Thing.php"],
        "the two nodes must carry the two files they came from"
    );
}

#[test]
fn ingesting_the_two_items_warns_naming_both_files() {
    let ks = Keyspace::new("php-duplicate-qname");
    let mut store = PetgraphStore::new();
    store
        .ingest_nodes(&ks, produced_nodes())
        .expect("ingest of produced nodes must succeed");

    let warnings = store.ingest_warnings(&ks);
    let contention: Vec<&cfdb_core::result::Warning> = warnings
        .iter()
        .filter(|w| w.kind == WarningKind::IdentityContention)
        .collect();
    assert!(
        !contention.is_empty(),
        "a qname declared in two files must not be resolved silently (RFC-054 §1), got: {warnings:?}"
    );
    let named_both = contention.iter().any(|w| {
        w.message.contains("src/legacy/Thing.php") && w.message.contains("src/modern/Thing.php")
    });
    assert!(
        named_both,
        "the warning must name both files, got: {contention:?}"
    );
}

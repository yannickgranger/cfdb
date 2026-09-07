use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use cfdb_core::fact::Node;
use cfdb_core::schema::schema_describe;
use cfdb_extractor_ts::TypeScriptProducer;
use cfdb_lang::LanguageProducer;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
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
fn every_attribute_the_typescript_producer_writes_is_declared() {
    let (nodes, _) = TypeScriptProducer
        .produce(&fixture("ts-richer"))
        .expect("TypeScriptProducer.produce over the ts-richer fixture");
    let labels: BTreeSet<&str> = nodes.iter().map(|n| n.label.as_str()).collect();
    assert_eq!(
        labels,
        BTreeSet::from(["CallSite", "Crate", "Item", "Module"]),
        "the fixture must reach every label this producer emits, or the assertion holds over a subset"
    );
    assert_eq!(
        undeclared(&nodes),
        BTreeMap::new(),
        "an attribute a producer writes but `schema_describe` does not declare is load-bearing and deniable at once: a rule can fence on it while `cfdb schema-describe` denies it exists (issue #693). Declare it in the descriptor rather than removing the emission."
    );
}

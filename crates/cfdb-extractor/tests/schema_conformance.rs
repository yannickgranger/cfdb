use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use cfdb_core::fact::Node;
use cfdb_core::schema::schema_describe;
use cfdb_extractor::extract_workspace;

fn declared() -> BTreeMap<String, BTreeSet<String>> {
    schema_describe()
        .nodes
        .into_iter()
        .map(|n| {
            (
                n.label.as_str().to_string(),
                n.attributes.into_iter().map(|a| a.name).collect(),
            )
        })
        .collect()
}

fn undeclared(nodes: &[Node]) -> BTreeMap<String, BTreeSet<String>> {
    let declared = declared();
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

fn cfdb_workspace_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("cfdb-extractor crate dir has parent (crates/)")
        .parent()
        .expect("crates/ has parent (cfdb sub-workspace root)")
}

#[test]
fn every_attribute_the_rust_producer_writes_is_declared() {
    let (nodes, _) = extract_workspace(cfdb_workspace_root()).expect("extract cfdb sub-workspace");
    assert!(
        !nodes.is_empty(),
        "the self workspace produced no node, so this assertion would hold vacuously"
    );
    assert_eq!(
        undeclared(&nodes),
        BTreeMap::new(),
        "an attribute a producer writes but `schema_describe` does not declare is load-bearing and deniable at once: a rule can fence on it while `cfdb schema-describe` denies it exists (issue #693). Declare it in the descriptor rather than removing the emission."
    );
}

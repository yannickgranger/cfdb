use cfdb_core::context_source::{parse_or_default, ContextSource};
use cfdb_core::schema::{Keyspace, Label};
use cfdb_petgraph::{persist, PetgraphStore};
use tempfile::tempdir;

#[test]
fn legacy_context_nodes_without_source_default_to_heuristic() {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("legacy-no-source.json");
    let legacy = r#"{
      "schema_version": { "major": 0, "minor": 2, "patch": 3 },
      "nodes": [
        {
          "id": "context:trading",
          "label": "Context",
          "props": {
            "name": "trading"
          }
        },
        {
          "id": "context:risk",
          "label": "Context",
          "props": {
            "name": "risk",
            "canonical_crate": "qbot-risk"
          }
        }
      ],
      "edges": []
    }"#;
    std::fs::write(&path, legacy).expect("tempdir is writable");

    let mut store = PetgraphStore::new();
    let ks = Keyspace::new("legacy");
    persist::load(&mut store, &ks, &path).expect("legacy keyspace must load");

    let (nodes, _edges) = store.export(&ks).expect("export legacy keyspace");
    let context_label = Label::new(Label::CONTEXT);
    let context_nodes: Vec<_> = nodes.iter().filter(|n| n.label == context_label).collect();
    assert_eq!(
        context_nodes.len(),
        2,
        "both legacy :Context nodes must round-trip"
    );

    for node in &context_nodes {
        let source = parse_or_default(node.props.get("source"));
        assert_eq!(
            source,
            ContextSource::Heuristic,
            "legacy :Context node {:?} must default to Heuristic — RFC-038 §4 \
             forbids promoting absence to Declared",
            node.id
        );
    }
}

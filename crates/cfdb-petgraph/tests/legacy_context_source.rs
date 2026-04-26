//! Legacy keyspace tolerance for `:Context.source` (RFC-038 slice 4 / #303).
//!
//! Pre-RFC-038 keyspaces on disk carry `:Context` nodes WITHOUT a `source`
//! prop. Post-RFC-038 readers MUST treat absence as
//! [`ContextSource::Heuristic`] per RFC-038 §4 — absence of provenance
//! cannot be promoted to declared status. This is the consumer-side scar
//! that pins the invariant: synthesise a legacy-shape `:Context` node, load
//! it through the persist layer, walk the loaded keyspace, and assert
//! [`parse_or_default`] returns `Heuristic` for every `:Context` node.
//!
//! Regressions would surface if (a) `parse_or_default` started defaulting
//! to `Declared`, (b) the persist layer started rejecting `:Context` nodes
//! without a `source` prop, or (c) the wire format mandated `source` such
//! that pre-RFC-038 keyspaces became unreadable.

use cfdb_core::context_source::{parse_or_default, ContextSource};
use cfdb_core::schema::{Keyspace, Label};
use cfdb_petgraph::{persist, PetgraphStore};
use tempfile::tempdir;

#[test]
fn legacy_context_nodes_without_source_default_to_heuristic() {
    // Hand-crafted pre-RFC-038 keyspace: two `:Context` nodes, neither
    // carrying `source`. The schema_version is the same v0.2 envelope used
    // throughout cfdb's history — RFC-038 added the `:Context.source`
    // attribute as a non-breaking additive field, so legacy files load
    // through the unchanged version gate.
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

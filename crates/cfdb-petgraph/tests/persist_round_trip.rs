use std::collections::BTreeMap;

use cfdb_core::fact::{Edge, Node, PropValue};
use cfdb_core::schema::{EdgeLabel, Keyspace, Label};
use cfdb_core::store::StoreBackend;
use cfdb_petgraph::{persist, PetgraphStore};
use tempfile::tempdir;

fn sample_nodes_edges() -> (Vec<Node>, Vec<Edge>) {
    let mut crate_props = BTreeMap::new();
    crate_props.insert("name".to_string(), PropValue::Str("qbot-domain".into()));

    let nodes = vec![
        Node {
            id: "crate:qbot-domain".into(),
            label: Label::new(Label::CRATE),
            props: crate_props,
        },
        Node {
            id: "item:qbot_domain::Order".into(),
            label: Label::new(Label::ITEM),
            props: {
                let mut p = BTreeMap::new();
                p.insert("qname".into(), PropValue::Str("qbot_domain::Order".into()));
                p.insert("kind".into(), PropValue::Str("struct".into()));
                p.insert("crate".into(), PropValue::Str("qbot-domain".into()));
                p.insert("line".into(), PropValue::Int(42));
                p
            },
        },
        Node {
            id: "item:qbot_domain::now_utc".into(),
            label: Label::new(Label::ITEM),
            props: {
                let mut p = BTreeMap::new();
                p.insert(
                    "qname".into(),
                    PropValue::Str("qbot_domain::now_utc".into()),
                );
                p.insert("kind".into(), PropValue::Str("fn".into()));
                p.insert("crate".into(), PropValue::Str("qbot-domain".into()));
                p
            },
        },
    ];

    let edges = vec![
        Edge {
            src: "item:qbot_domain::Order".into(),
            dst: "crate:qbot-domain".into(),
            label: EdgeLabel::new(EdgeLabel::IN_CRATE),
            props: BTreeMap::new(),
        },
        Edge {
            src: "item:qbot_domain::now_utc".into(),
            dst: "crate:qbot-domain".into(),
            label: EdgeLabel::new(EdgeLabel::IN_CRATE),
            props: BTreeMap::new(),
        },
    ];

    (nodes, edges)
}

#[test]
fn save_then_load_preserves_canonical_dump() {
    let ks = Keyspace::new("test-ks");
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("test-ks.json");

    let mut store_a = PetgraphStore::new();
    let (nodes, edges) = sample_nodes_edges();
    store_a
        .ingest_nodes(&ks, nodes)
        .expect("ingest into fresh store is infallible");
    store_a
        .ingest_edges(&ks, edges)
        .expect("ingest into fresh store is infallible");
    persist::save(&store_a, &ks, &path).expect("save");
    let dump_before = store_a
        .canonical_dump(&ks)
        .expect("canonical_dump over populated store is infallible");

    let mut store_b = PetgraphStore::new();
    persist::load(&mut store_b, &ks, &path).expect("load");
    let dump_after = store_b
        .canonical_dump(&ks)
        .expect("canonical_dump over loaded store is infallible");

    assert_eq!(
        dump_before, dump_after,
        "canonical dump differs after save/load round-trip"
    );
}

#[test]
fn save_is_byte_identical_across_two_calls() {
    let ks = Keyspace::new("det");
    let dir = tempdir().expect("tempdir");
    let path_a = dir.path().join("a.json");
    let path_b = dir.path().join("b.json");

    let mut store = PetgraphStore::new();
    let (nodes, edges) = sample_nodes_edges();
    store
        .ingest_nodes(&ks, nodes)
        .expect("ingest into fresh store is infallible");
    store
        .ingest_edges(&ks, edges)
        .expect("ingest into fresh store is infallible");

    persist::save(&store, &ks, &path_a).expect("save a");
    persist::save(&store, &ks, &path_b).expect("save b");

    let bytes_a = std::fs::read(&path_a).expect("just-written fixture file is readable");
    let bytes_b = std::fs::read(&path_b).expect("just-written fixture file is readable");
    assert_eq!(
        bytes_a, bytes_b,
        "two save calls must be byte-identical (G1)"
    );
}

#[test]
fn load_rejects_incompatible_schema_version() {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("bad.json");
    let bad = r#"{
      "schema_version": { "major": 99, "minor": 0, "patch": 0 },
      "nodes": [],
      "edges": []
    }"#;
    std::fs::write(&path, bad).expect("tempdir is writable");

    let mut store = PetgraphStore::new();
    let err = persist::load(&mut store, &Keyspace::new("x"), &path);
    assert!(
        matches!(err, Err(cfdb_core::StoreError::SchemaMismatch { .. })),
        "expected SchemaMismatch, got {:?}",
        err
    );
}

#[test]
fn load_accepts_legacy_v0_2_keyspace_with_no_index_fields() {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("legacy.json");
    let legacy = r#"{
      "schema_version": { "major": 0, "minor": 2, "patch": 2 },
      "nodes": [
        {
          "id": "item:legacy_crate::Foo",
          "label": "Item",
          "props": {
            "qname": "legacy_crate::Foo",
            "kind": "struct",
            "crate": "legacy-crate"
          }
        },
        {
          "id": "item:legacy_crate::bar",
          "label": "Item",
          "props": {
            "qname": "legacy_crate::bar",
            "kind": "fn",
            "crate": "legacy-crate"
          }
        }
      ],
      "edges": []
    }"#;
    std::fs::write(&path, legacy).expect("tempdir is writable");

    let mut store = PetgraphStore::new();
    let ks = Keyspace::new("legacy");
    persist::load(&mut store, &ks, &path).expect("legacy v0.2 keyspace must load");

    let (nodes, edges) = store.export(&ks).expect("export legacy keyspace");
    assert_eq!(nodes.len(), 2, "both legacy nodes must round-trip");
    assert_eq!(edges.len(), 0, "no edges in fixture");

    let dump = store
        .canonical_dump(&ks)
        .expect("canonical_dump over loaded legacy keyspace");
    assert!(
        !dump.is_empty(),
        "canonical_dump must reflect the loaded legacy state"
    );
    assert!(
        dump.contains("legacy_crate::Foo"),
        "canonical_dump must include the legacy item qnames"
    );
}

#[test]
fn saved_keyspace_is_compact_json() {
    let ks = Keyspace::new("compact-ks");
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("compact-ks.json");

    let mut store = PetgraphStore::new();
    let (nodes, edges) = sample_nodes_edges();
    store.ingest_nodes(&ks, nodes).expect("ingest nodes");
    store.ingest_edges(&ks, edges).expect("ingest edges");
    persist::save(&store, &ks, &path).expect("save");

    let bytes = std::fs::read(&path).expect("read saved keyspace");
    assert!(
        !bytes.contains(&b'\n'),
        "keyspace file is pretty-printed — expected compact JSON \
         (single line, no indentation)"
    );
}

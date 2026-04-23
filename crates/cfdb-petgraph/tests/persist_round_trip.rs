//! Persistence round-trip: save a keyspace to disk, drop it, load it back,
//! and assert the canonical dump is byte-identical. This is the integration
//! test that backs the CLI's extract → query file handoff (RFC §11).

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

    // Build, save.
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

    // Load into a fresh store.
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
    // Hand-craft a file with a major version we can't read.
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

/// AC3 (RFC-035 slice 4 / #183) — legacy v0.2 keyspaces written
/// before the index subsystem landed MUST load bit-identically. The
/// wire format is unchanged (RFC-035 §3.7); the file carries only
/// `schema_version` + `nodes` + `edges` (no `by_prop`, no
/// `index_spec`). `persist::load` must accept any version `can_read`
/// allows — i.e. same-major and `<= CURRENT`.
///
/// This test pins a hand-crafted V0_2_2 envelope (one minor below
/// CURRENT V0_2_3 at slice 4 merge time) with two `:Item` nodes and
/// asserts: load returns `Ok(())`; the loaded keyspace exposes both
/// nodes via `export`; `canonical_dump` is non-empty. Regressions
/// would surface if either (a) the version gate accidentally rejects
/// a legacy compatible version, or (b) future code adds an
/// unconditional read of an index-related field that breaks the
/// legacy shape.
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

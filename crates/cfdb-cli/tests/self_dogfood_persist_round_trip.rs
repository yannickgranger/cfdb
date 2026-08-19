use std::path::PathBuf;

use cfdb_core::schema::Keyspace;
use cfdb_core::store::StoreBackend;
use cfdb_petgraph::{persist, PetgraphStore};

fn cfdb_workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("CARGO_MANIFEST_DIR must have two parents")
        .to_path_buf()
}

#[test]
fn persist_round_trip_rebuilds_by_prop_on_cfdb_workspace() {
    let workspace = cfdb_workspace_root();
    let (nodes, edges) = cfdb_extractor::extract_workspace(&workspace).expect("extract cfdb");
    assert!(!nodes.is_empty(), "cfdb extract produced zero nodes");

    let ks = Keyspace::new("slice4_selfdog");

    let mut store_a = PetgraphStore::new();
    store_a.ingest_nodes(&ks, nodes).expect("ingest nodes");
    store_a.ingest_edges(&ks, edges).expect("ingest edges");
    let (nodes_a, edges_a) = store_a.export(&ks).expect("export source keyspace");
    let dump_a = store_a
        .canonical_dump(&ks)
        .expect("canonical_dump source store");

    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join(format!("{}.json", ks.as_str()));
    persist::save(&store_a, &ks, &path).expect("persist::save");
    let bytes_written = std::fs::metadata(&path).expect("save file exists").len();
    assert!(
        bytes_written > 0,
        "persist::save produced an empty file at {path:?}"
    );

    let mut store_b = PetgraphStore::new();
    persist::load(&mut store_b, &ks, &path).expect("persist::load");
    let dump_b = store_b
        .canonical_dump(&ks)
        .expect("canonical_dump loaded store");

    assert_eq!(
        dump_a, dump_b,
        "canonical_dump diverged after persist::save → persist::load round-trip"
    );

    let (nodes_b, edges_b) = store_b.export(&ks).expect("export loaded keyspace");
    assert_eq!(
        nodes_b.len(),
        nodes_a.len(),
        "node count diverged after round-trip"
    );
    assert_eq!(
        edges_b.len(),
        edges_a.len(),
        "edge count diverged after round-trip"
    );
    assert!(
        !nodes_a.is_empty(),
        "cfdb extract + ingest produced zero unique nodes"
    );
}

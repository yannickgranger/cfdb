//! Self-dogfood test for RFC-035 slice 4 (#183).
//!
//! Extracts cfdb's own worktree, ingests into a `PetgraphStore`,
//! saves the keyspace via `persist::save`, loads into a fresh store
//! via `persist::load`, and asserts the canonical-dump equality
//! across the round-trip. Defends the §3.7 invariant: indexes are
//! NOT serialised to disk and `by_prop` is rebuilt on every load
//! from the in-memory fact content via slice 2's `ingest_nodes`
//! pipeline.
//!
//! At slice 4 the destination keyspace is created with
//! `IndexSpec::empty()` (the `with_indexes` composition-root wiring
//! lands in slice 7). The empty-spec round-trip thus exercises the
//! load → ingest → by_prop-rebuild flow at scale (cfdb's full
//! keyspace, ~12 k nodes / ~12 k edges) and proves the round-trip
//! is content-identical.
//!
//! Lives in `cfdb-cli/tests/` because `cfdb-petgraph` may not depend
//! on `cfdb-extractor` per `crates/cfdb-petgraph/tests/architecture_dep_rule.rs`.
//! Same module-relative workspace-root pattern as the slice 3
//! self-dogfood (`self_dogfood_computed_key_evaluate.rs`).

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

    // SOURCE: build store, ingest, snapshot canonical_dump. The
    // extractor may emit multiple `Node` records that share an `id`
    // (e.g. a re-emitted item across passes); `KeyspaceState`'s
    // `id_to_idx` deduplicates by id (last write wins). The
    // round-trip count assertion below uses the deduplicated counts
    // from `export`, NOT the raw extract counts.
    let mut store_a = PetgraphStore::new();
    store_a.ingest_nodes(&ks, nodes).expect("ingest nodes");
    store_a.ingest_edges(&ks, edges).expect("ingest edges");
    let (nodes_a, edges_a) = store_a.export(&ks).expect("export source keyspace");
    let dump_a = store_a
        .canonical_dump(&ks)
        .expect("canonical_dump source store");

    // SAVE → tmp file.
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join(format!("{}.json", ks.as_str()));
    persist::save(&store_a, &ks, &path).expect("persist::save");
    let bytes_written = std::fs::metadata(&path).expect("save file exists").len();
    assert!(
        bytes_written > 0,
        "persist::save produced an empty file at {path:?}"
    );

    // DESTINATION: fresh store + load.
    let mut store_b = PetgraphStore::new();
    persist::load(&mut store_b, &ks, &path).expect("persist::load");
    let dump_b = store_b
        .canonical_dump(&ks)
        .expect("canonical_dump loaded store");

    // Outer invariant — round-trip is content-identical at the canonical layer.
    assert_eq!(
        dump_a, dump_b,
        "canonical_dump diverged after persist::save → persist::load round-trip"
    );

    // Inner check — deduplicated node + edge counts survive the round-trip.
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

//! AC1 round-trip test for RFC-035 slice 4 (#183).
//!
//! Lives in its own `#[cfg(test)] mod` file (declared from `lib.rs`)
//! to keep `crates/cfdb-petgraph/src/graph.rs` under the workspace
//! god-file ceiling — the test wants visibility into `pub(crate)`
//! `KeyspaceState` + `PetgraphStore::keyspaces`, both of which are
//! crate-visible by construction so a sibling test module reaches
//! them without any new public surface (Forbidden Move #11 honored).
//!
//! Test-helper duplication (`item`, `three_index_spec`) is acceptable
//! here because it is intentionally inert — these helpers exist only
//! to construct fixture nodes for this one round-trip assertion. They
//! mirror the equivalent helpers in `graph::index_build_tests`.

use cfdb_core::fact::Node;
use cfdb_core::schema::{Keyspace, Label};
use cfdb_core::store::StoreBackend;

use crate::graph::KeyspaceState;
use crate::index::spec::{ComputedKey, IndexEntry, IndexSpec};
use crate::PetgraphStore;

fn three_index_spec() -> IndexSpec {
    IndexSpec {
        entries: vec![
            IndexEntry::Prop {
                label: "Item".into(),
                prop: "qname".into(),
                notes: "test".into(),
            },
            IndexEntry::Prop {
                label: "Item".into(),
                prop: "bounded_context".into(),
                notes: "test".into(),
            },
            IndexEntry::Computed {
                label: "Item".into(),
                computed: ComputedKey::LastSegment,
                notes: "test".into(),
            },
        ],
    }
}

fn item(id: &str, qname: &str, ctx: &str) -> Node {
    Node::new(id, Label::new("Item"))
        .with_prop("qname", qname)
        .with_prop("bounded_context", ctx)
}

/// AC1 (RFC-035 slice 4 / #183) — `persist::load` rebuilds
/// `by_prop` from the in-memory fact content via slice 2's
/// `ingest_nodes` chain. The rebuild MUST be byte-for-byte
/// identical to the ingest-time `by_prop` when the destination
/// keyspace carries the same `IndexSpec` as the source.
///
/// Mechanism (already shipped in slice 2 / commit 55fcd88):
/// `persist::load → PetgraphStore::ingest_nodes → keyspace_mut →
/// KeyspaceState::ingest_one_node → compute_index_entries`
/// populates `by_prop` from `self.index_spec`. Slice 4 verifies
/// the round-trip dimension that slice 2 didn't explicitly test.
///
/// Non-empty-spec test surface: we pre-seed `store.keyspaces`
/// directly via the `pub(crate) keyspaces` field — this stays
/// inside `#[cfg(test)] mod` access (no public surface added per
/// Forbidden Move #11; the slice 7 `with_indexes` builder is
/// still pending).
#[test]
fn by_prop_rebuilt_on_load_matches_ingest_time_state() {
    let spec = three_index_spec();
    let ks = Keyspace::new("rt-rebuild");
    let nodes = vec![
        item("item:a", "alpha::foo_1", "context_a"),
        item("item:b", "beta::bar_2", "context_b"),
        item("item:c", "gamma::baz_3", "context_c"),
        item("item:d", "delta::qux_4", "context_a"),
        item("item:e", "alpha::quux_5", "context_b"),
    ];

    // SOURCE: build a store whose keyspace carries the spec.
    let mut store_a = PetgraphStore::new();
    store_a
        .keyspaces
        .insert(ks.clone(), KeyspaceState::new_with_spec(spec.clone()));
    store_a.ingest_nodes(&ks, nodes.clone()).expect("ingest");
    let by_prop_before = store_a
        .keyspaces
        .get(&ks)
        .expect("keyspace present")
        .by_prop
        .clone();
    assert!(
        !by_prop_before.is_empty(),
        "ingest with non-empty spec must populate by_prop"
    );

    // SAVE → tmp file.
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("rt-rebuild.json");
    crate::persist::save(&store_a, &ks, &path).expect("save");

    // DESTINATION: fresh store + same-spec keyspace pre-seeded.
    let mut store_b = PetgraphStore::new();
    store_b
        .keyspaces
        .insert(ks.clone(), KeyspaceState::new_with_spec(spec));

    // LOAD → automatic by_prop rebuild via the slice-2 ingest pipeline.
    crate::persist::load(&mut store_b, &ks, &path).expect("load");

    let by_prop_after = &store_b
        .keyspaces
        .get(&ks)
        .expect("keyspace present after load")
        .by_prop;

    assert_eq!(
        &by_prop_before, by_prop_after,
        "by_prop after load must match ingest-time by_prop byte-for-byte"
    );

    // Defence-in-depth: the canonical_dump must also match (proves
    // the underlying graph state round-tripped, not just the index).
    let dump_a = store_a.canonical_dump(&ks).expect("dump a");
    let dump_b = store_b.canonical_dump(&ks).expect("dump b");
    assert_eq!(dump_a, dump_b);
}

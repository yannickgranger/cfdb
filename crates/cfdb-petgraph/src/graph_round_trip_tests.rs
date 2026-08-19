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

    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("rt-rebuild.json");
    crate::persist::save(&store_a, &ks, &path).expect("save");

    let mut store_b = PetgraphStore::new();
    store_b
        .keyspaces
        .insert(ks.clone(), KeyspaceState::new_with_spec(spec));

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

    let dump_a = store_a.canonical_dump(&ks).expect("dump a");
    let dump_b = store_b.canonical_dump(&ks).expect("dump b");
    assert_eq!(dump_a, dump_b);
}

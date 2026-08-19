use cfdb_core::fact::Node;
use cfdb_core::schema::{Keyspace, Label};
use cfdb_core::store::StoreBackend;

use crate::index::spec::{ComputedKey, IndexEntry, IndexSpec};
use crate::PetgraphStore;

fn slice7_spec() -> IndexSpec {
    IndexSpec {
        entries: vec![
            IndexEntry::Prop {
                label: "Item".into(),
                prop: "qname".into(),
                notes: "slice-7 propagation".into(),
            },
            IndexEntry::Computed {
                label: "Item".into(),
                computed: ComputedKey::LastSegment,
                notes: "slice-7 propagation".into(),
            },
        ],
    }
}

fn item(id: &str, qname: &str) -> Node {
    Node::new(id, Label::new("Item")).with_prop("qname", qname)
}

fn ks() -> Keyspace {
    Keyspace::new("slice7-propagation")
}

#[test]
fn with_indexes_stores_spec_on_store() {
    let spec = slice7_spec();
    let store = PetgraphStore::new().with_indexes(spec.clone());
    assert_eq!(
        store.index_spec, spec,
        "with_indexes must carry the spec on PetgraphStore.index_spec"
    );
}

#[test]
fn default_store_has_empty_index_spec() {
    let store = PetgraphStore::new();
    assert!(
        store.index_spec.is_empty(),
        "a fresh PetgraphStore must have an empty IndexSpec — existing \
         callers must keep identical pre-slice-7 behaviour"
    );
}

#[test]
fn keyspace_mut_propagates_spec_to_auto_created_keyspaces() {
    let mut store = PetgraphStore::new().with_indexes(slice7_spec());
    store
        .ingest_nodes(&ks(), vec![item("item:a", "foo::bar")])
        .expect("ingest");
    let state = store
        .keyspaces
        .get(&ks())
        .expect("keyspace was auto-created by ingest_nodes via keyspace_mut");
    assert!(
        !state.by_prop.is_empty(),
        "by_prop must be populated after ingest when the store carries a \
         non-empty IndexSpec — slice-7 propagation gap regression"
    );
    let qname_bucket = state
        .by_prop
        .iter()
        .find_map(|((label, tag), map)| {
            if label.as_str() == "Item" && tag.as_str() == "qname" {
                Some(map)
            } else {
                None
            }
        })
        .expect("(Item, qname) posting list present");
    assert_eq!(
        qname_bucket.len(),
        1,
        "(Item, qname) bucket should contain one distinct value for the one ingested item"
    );
    let last_seg_bucket_count = state
        .by_prop
        .iter()
        .filter(|((label, tag), _)| {
            label.as_str() == "Item" && tag.as_str() == "last_segment(qname)"
        })
        .count();
    assert_eq!(
        last_seg_bucket_count, 1,
        "computed-key (Item, last_segment(qname)) posting list must also exist — \
         proves the spec's second entry flowed through"
    );
}

#[test]
fn default_store_leaves_by_prop_empty_after_ingest() {
    let mut store = PetgraphStore::new();
    store
        .ingest_nodes(&ks(), vec![item("item:a", "foo::bar")])
        .expect("ingest");
    let state = store
        .keyspaces
        .get(&ks())
        .expect("keyspace auto-created on ingest");
    assert!(
        state.by_prop.is_empty(),
        "by_prop must stay empty when the store was built without with_indexes"
    );
}

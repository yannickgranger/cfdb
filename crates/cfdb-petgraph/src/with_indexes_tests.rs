//! Slice-7 (#186) — `PetgraphStore::with_indexes` builder + `keyspace_mut`
//! spec propagation.
//!
//! The slice closes the spec-flow gap between `cfdb-cli::compose::load_store*`
//! and per-keyspace `by_prop` posting lists: a store constructed via
//! `PetgraphStore::new().with_indexes(spec)` carries the spec on its
//! field, and `keyspace_mut` threads that spec into every auto-created
//! [`KeyspaceState`] so `ingest_nodes` populates `by_prop` without any
//! caller touching `KeyspaceState::new_with_spec` directly.
//!
//! These tests sit in a sibling `#[cfg(test)] mod` (declared from
//! `lib.rs`) so they can reach `pub(crate)` items (`keyspaces`,
//! `IndexTag`, `IndexValue`) without widening the public surface.

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
    // Two expected posting-list buckets: (Item, "qname") and
    // (Item, "last_segment(qname)"). Non-empty by_prop proves the spec
    // flowed all the way from `with_indexes` to the keyspace's ingest
    // path (RFC-035 §3.8).
    assert!(
        !state.by_prop.is_empty(),
        "by_prop must be populated after ingest when the store carries a \
         non-empty IndexSpec — slice-7 propagation gap regression"
    );
    // Inspect the canonical prop bucket: last_segment("foo::bar") = "bar"
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
    // The inverse of the slice-7 propagation test — a store constructed
    // WITHOUT `with_indexes` must leave `by_prop` empty across ingest,
    // matching pre-slice-7 behaviour. Guards against accidental always-on
    // indexing that would defeat the opt-in `.cfdb/indexes.toml` contract.
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

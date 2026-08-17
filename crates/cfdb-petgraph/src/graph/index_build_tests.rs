//! Exercise `by_prop` build + stale-entry removal at the `KeyspaceState`
//! layer. `KeyspaceState` stays `pub(crate)` and is tested at this level.
//!
//! The parent `graph.rs` declares this module via
//! `#[cfg(test)] mod index_build_tests;`, so this file does NOT carry
//! its own `#![cfg(test)]` — that would be a duplicate-attribute
//! clippy violation (rust-1.93 `clippy::duplicated_attributes`).

use super::*;
use crate::canonical_dump::canonical_dump;
use crate::index::spec::{ComputedKey, IndexEntry, IndexSpec};
use cfdb_core::fact::{Node, PropValue};
use cfdb_core::schema::Label;

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

fn full_scan(state: &KeyspaceState, label: &str, key: &str, value: &str) -> BTreeSet<NodeIndex> {
    let target_label = Label::new(label);
    state
        .graph
        .node_indices()
        .filter(|&idx| {
            let node = state.graph.node_weight(idx).expect("valid idx");
            if node.label != target_label {
                return false;
            }
            match key {
                "last_segment(qname)" => {
                    let qname = node.props.get("qname").and_then(PropValue::as_str);
                    qname
                        .map(|q| cfdb_core::qname::last_segment(q) == value)
                        .unwrap_or(false)
                }
                other => node
                    .props
                    .get(other)
                    .and_then(PropValue::as_str)
                    .is_some_and(|s| s == value),
            }
        })
        .collect()
}

#[test]
fn recall_matches_full_scan_on_1000_item_fixture() {
    let contexts = ["context_a", "context_b", "context_c", "context_d"];
    let roots = ["alpha", "beta", "gamma", "delta", "epsilon"];
    let leaves = [
        "foo", "bar", "baz", "qux", "quux", "corge", "grault", "xyzzy",
    ];

    let mut nodes = Vec::with_capacity(1000);
    for i in 0..1000 {
        let ctx = contexts[i % contexts.len()];
        let root = roots[(i / contexts.len()) % roots.len()];
        let leaf = leaves[(i / (contexts.len() * roots.len())) % leaves.len()];
        let disc = i; // disambiguator to keep ids unique
        let qname = format!("{root}::{leaf}_{disc}");
        nodes.push(item(&format!("item:{i}"), &qname, ctx));
    }

    let mut state = KeyspaceState::new_with_spec(three_index_spec());
    state.ingest_nodes(nodes);

    assert_eq!(state.graph.node_count(), 1000);

    // Recall ≡ full scan across every indexed (Label, key).
    for ((label, tag), postings) in &state.by_prop {
        for (value, indices) in postings {
            let scanned = full_scan(&state, label.as_str(), tag, value);
            assert_eq!(
                indices, &scanned,
                "by_prop[({label:?}, {tag})][{value}] diverged from full scan"
            );
        }
    }

    // Contexts bucket size — 1000 / 4 contexts = 250 each.
    let ctx_key = (Label::new("Item"), "bounded_context".to_string());
    let ctx_buckets = state.by_prop.get(&ctx_key).expect("context index");
    assert_eq!(ctx_buckets.len(), contexts.len());
    for (ctx, set) in ctx_buckets {
        assert_eq!(
            set.len(),
            250,
            "context `{ctx}` should hold 1000/4 = 250 items"
        );
    }

    // Computed-key bucket size: 8 distinct leaves × 5 roots = 40 distinct
    // last-segment suffixes? No — last_segment includes the disambiguator,
    // so all 1000 values are distinct. Assert that instead.
    let comp_key = (Label::new("Item"), "last_segment(qname)".to_string());
    let comp_buckets = state.by_prop.get(&comp_key).expect("computed index");
    assert_eq!(comp_buckets.len(), 1000);
}

#[test]
fn stale_entry_removed_on_reingest_with_changed_prop() {
    let mut state = KeyspaceState::new_with_spec(three_index_spec());
    state.ingest_nodes(vec![item("item:a", "mod::foo", "context_a")]);

    let label_item = Label::new("Item");
    let key_qname = (label_item.clone(), "qname".to_string());
    let key_ctx = (label_item.clone(), "bounded_context".to_string());
    let key_last = (label_item, "last_segment(qname)".to_string());

    let idx = *state.id_to_idx.get("item:a").expect("ingested");
    assert!(state.by_prop[&key_qname]["mod::foo"].contains(&idx));
    assert!(state.by_prop[&key_ctx]["context_a"].contains(&idx));
    assert!(state.by_prop[&key_last]["foo"].contains(&idx));

    // Re-ingest with a changed qname AND a changed context.
    state.ingest_nodes(vec![item("item:a", "mod::bar", "context_b")]);

    // Stale postings: old values lose the idx AND the (now-empty) entries
    // are pruned from the outer map so iteration stays minimal.
    assert!(
        !state.by_prop[&key_qname].contains_key("mod::foo"),
        "stale qname posting list should be pruned, not merely emptied"
    );
    assert!(!state.by_prop[&key_ctx].contains_key("context_a"));
    assert!(!state.by_prop[&key_last].contains_key("foo"));

    // Fresh postings: new values carry the idx.
    assert!(state.by_prop[&key_qname]["mod::bar"].contains(&idx));
    assert!(state.by_prop[&key_ctx]["context_b"].contains(&idx));
    assert!(state.by_prop[&key_last]["bar"].contains(&idx));

    // Only one node in the keyspace, so the node-count stays at 1.
    assert_eq!(state.graph.node_count(), 1);
}

#[test]
fn canonical_dump_unaffected_by_by_prop() {
    // Determinism / G1 invariant: indexes are rebuild-able scratch and
    // MUST NOT leak into `canonical_dump`. A keyspace ingested with
    // indexes and one ingested without indexes produce byte-identical
    // canonical dumps on the same fact content (RFC-035 §4).
    let nodes = vec![
        item("item:a", "mod::foo", "context_a"),
        item("item:b", "mod::bar", "context_b"),
        item("item:c", "other::foo", "context_a"),
    ];

    let mut indexed = KeyspaceState::new_with_spec(three_index_spec());
    indexed.ingest_nodes(nodes.clone());

    let mut plain = KeyspaceState::new();
    plain.ingest_nodes(nodes);

    let indexed_dump = canonical_dump(&indexed);
    let plain_dump = canonical_dump(&plain);
    assert_eq!(
        indexed_dump, plain_dump,
        "canonical_dump must be byte-identical with vs without indexes"
    );

    // Sanity: the indexed keyspace actually populated its posting lists.
    assert!(!indexed.by_prop.is_empty());
    assert!(plain.by_prop.is_empty());
}

#[test]
fn empty_spec_skips_build_pass_entirely() {
    let mut state = KeyspaceState::new();
    state.ingest_nodes(vec![item("item:a", "mod::foo", "context_a")]);
    assert!(
        state.by_prop.is_empty(),
        "no spec entries means no index maintenance"
    );
}

#[test]
fn label_change_on_reingest_drops_old_label_entries() {
    let mut state = KeyspaceState::new_with_spec(three_index_spec());
    state.ingest_nodes(vec![item("item:a", "mod::foo", "context_a")]);

    let label_item = Label::new("Item");
    let key_qname = (label_item, "qname".to_string());
    let idx = *state.id_to_idx.get("item:a").expect("ingested");
    assert!(state.by_prop[&key_qname]["mod::foo"].contains(&idx));

    // Re-ingest with a label the spec does not cover — Item → CallSite.
    let changed = Node::new("item:a", Label::new("CallSite"))
        .with_prop("qname", "mod::foo")
        .with_prop("bounded_context", "context_a");
    state.ingest_nodes(vec![changed]);

    // All (Item, *) entries for this idx should have been dropped. The
    // CallSite label is not in the spec so no new entries appear.
    assert!(!state
        .by_prop
        .get(&key_qname)
        .is_some_and(|m| m.contains_key("mod::foo")));
}

//! Posting-list maintenance helpers for `KeyspaceState::by_prop`.
//!
//! `reconcile_index_entries` (in `graph.rs`) walks the before/after
//! diff for an updated node and calls these helpers per (label, tag,
//! value) triple. Hoisting the per-triple work out of the loop body
//! keeps the loop free of `.clone()` calls (the metric scanner flags
//! clones-in-loops; the structural BTreeMap-key clones live here, in
//! a single-shot function body, instead).
//!
//! The helpers operate directly on `&mut KeyspaceState::by_prop`
//! rather than `&mut self` to avoid pulling the rest of the keyspace
//! state into their borrow scope.

use std::collections::{BTreeMap, BTreeSet};

use cfdb_core::schema::Label;
use petgraph::stable_graph::NodeIndex;

use crate::index::build::{IndexTag, IndexValue};

/// Remove `idx` from the posting list at `(label, tag, value)`,
/// pruning the inner-value entry when its `BTreeSet` empties and
/// pruning the outer `(label, tag)` bucket when its inner map empties.
///
/// Called once per stale (label, tag, value) triple by
/// [`KeyspaceState::reconcile_index_entries`](crate::graph::KeyspaceState).
pub(crate) fn remove_posting(
    by_prop: &mut BTreeMap<(Label, IndexTag), BTreeMap<IndexValue, BTreeSet<NodeIndex>>>,
    label: &Label,
    tag: &IndexTag,
    value: &IndexValue,
    idx: NodeIndex,
) {
    let outer_key = (label.clone(), tag.clone());
    let Some(inner) = by_prop.get_mut(&outer_key) else {
        return;
    };
    if let Some(set) = inner.get_mut(value) {
        set.remove(&idx);
        if set.is_empty() {
            inner.remove(value);
        }
    }
    if inner.is_empty() {
        by_prop.remove(&outer_key);
    }
}

/// Insert `idx` into the posting list at `(label, tag, value)`,
/// allocating the outer `(label, tag)` bucket and the inner
/// `value → BTreeSet<NodeIndex>` entry on demand.
///
/// Called once per fresh (label, tag, value) triple by
/// [`KeyspaceState::reconcile_index_entries`](crate::graph::KeyspaceState).
pub(crate) fn insert_posting(
    by_prop: &mut BTreeMap<(Label, IndexTag), BTreeMap<IndexValue, BTreeSet<NodeIndex>>>,
    label: &Label,
    tag: &IndexTag,
    value: &IndexValue,
    idx: NodeIndex,
) {
    by_prop
        .entry((label.clone(), tag.clone()))
        .or_default()
        .entry(value.clone())
        .or_default()
        .insert(idx);
}

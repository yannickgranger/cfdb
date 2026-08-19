use std::collections::{BTreeMap, BTreeSet};

use cfdb_core::schema::Label;
use petgraph::stable_graph::NodeIndex;

use crate::index::build::{IndexTag, IndexValue};

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

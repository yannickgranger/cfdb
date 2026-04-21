//! `enrich_reachability` — BFS from every `:EntryPoint` over `CALLS*` edges,
//! writing `:Item.reachable_from_entry` (bool) + `:Item.reachable_entry_count`
//! (i64) per item (slice 43-G / issue #110).
//!
//! # Algorithm (rust-systems Q4)
//!
//! 1. **Seed set** — every `(:EntryPoint)-[:EXPOSES]->(:Item)` target is a
//!    handler item. Seeds are sorted by `NodeIndex` wrapped in a `BTreeSet`
//!    for deterministic iteration.
//! 2. **Per-seed BFS** — for each seed, walk outgoing `CALLS` **and**
//!    `INVOKES_AT` edges until the frontier is exhausted. Both edge kinds
//!    are needed because the HIR extractor models dispatch as
//!    `(:Item)-[:INVOKES_AT]->(:CallSite)-[:CALLS]->(:Item)` (the two-hop
//!    path represents "this item invokes that callsite which resolves to
//!    that callee"); the syn-only path is `(:Item)-[:CALLS]->(:Item)`
//!    direct (no callsite intermediate). Walking both covers both shapes
//!    and lets the BFS traverse a mixed graph without distinguishing them.
//!    Track visited via `BTreeSet<NodeIndex>`.
//! 3. **Attribution** — a `BTreeMap<NodeIndex, i64>` counts how many
//!    distinct seeds reach each node. Only `:Item` nodes are attributed;
//!    transitively-visited `:CallSite` nodes are ignored at count time.
//! 4. **Write attrs** — every `:Item` node gets both attrs. Items with
//!    `count == 0` are explicitly marked `reachable_from_entry = false,
//!    reachable_entry_count = 0` — never silently left null.
//!
//! # Degraded path (clean-arch B3)
//!
//! If the keyspace carries zero `:EntryPoint` nodes, the pass returns
//! `ran: false` with a clear warning naming `cfdb extract --features hir`.
//! **Never** silently mark every item unreachable in this case — the
//! classifier would misread that as "everything is unwired," which is
//! factually wrong (it just means the HIR pass that populates entry
//! points didn't run).
//!
//! # Determinism (AC-6)
//!
//! - Seed collection uses `BTreeSet<NodeIndex>` sorted by index.
//! - Per-seed BFS visits via `BTreeSet<NodeIndex>`; iteration at
//!   attribution time follows sorted-index order.
//! - `reach_count` is a `BTreeMap<NodeIndex, i64>`.
//! - Attribute writes iterate `nodes_with_label` which returns a sorted
//!   `Vec<NodeIndex>`.
//!
//! Two runs on the same graph produce byte-identical canonical dumps.
//!
//! # Cycle safety (AC-5)
//!
//! BFS terminates because each visited node is recorded in the `BTreeSet`
//! before its outgoing edges are walked. A cycle `A → B → A` visits A
//! once, queues B, visits B, attempts to queue A (already visited, not
//! re-added), and the frontier drains.
//!
//! # Accuracy caveat
//!
//! `reachable_from_entry = false` is only as accurate as the `CALLS`
//! edges populated by `cfdb-hir-extractor` (RFC v0.2-4 targets ≥80%
//! recall). The classifier (#48) applies confidence gating on the
//! "Unwired" class accordingly.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use cfdb_core::enrich::EnrichReport;
use cfdb_core::fact::PropValue;
use cfdb_core::schema::{EdgeLabel, Label};
use petgraph::stable_graph::NodeIndex;
use petgraph::visit::EdgeRef;
use petgraph::Direction;

use crate::graph::KeyspaceState;

pub(crate) const VERB: &str = "enrich_reachability";
const ATTR_REACHABLE: &str = "reachable_from_entry";
const ATTR_COUNT: &str = "reachable_entry_count";

pub(crate) fn run(state: &mut KeyspaceState) -> EnrichReport {
    let entry_points = state.nodes_with_label(&Label::new(Label::ENTRY_POINT));

    // Degraded path — refuse to mark every item `reachable_from_entry = false`
    // when there are no entry points. See clean-arch B3 in council/43.
    if entry_points.is_empty() {
        return EnrichReport {
            verb: VERB.into(),
            ran: false,
            facts_scanned: 0,
            attrs_written: 0,
            edges_written: 0,
            warnings: vec![
                "enrich_reachability: no :EntryPoint nodes in keyspace — run `cfdb extract --features hir` first to populate entry points before reachability enrichment".into(),
            ],
        };
    }

    let seeds = collect_seeds(state, &entry_points);
    let reach_count = accumulate_reach_counts(state, &seeds);
    let attrs_written = write_item_attrs(state, &reach_count);

    EnrichReport {
        verb: VERB.into(),
        ran: true,
        facts_scanned: u64::try_from(entry_points.len()).unwrap_or(u64::MAX),
        attrs_written,
        edges_written: 0,
        warnings: Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// Seed collection
// ---------------------------------------------------------------------------

/// Collect the set of `:Item` `NodeIndex`es that are EXPOSES-targets of some
/// `:EntryPoint`. An entry point with no outgoing EXPOSES edge contributes
/// no seed (the catalog is inconsistent, but we don't fail — the classifier
/// still gets useful data from the entry points that DO expose).
fn collect_seeds(state: &KeyspaceState, entry_points: &[NodeIndex]) -> BTreeSet<NodeIndex> {
    entry_points
        .iter()
        .flat_map(|&ep_idx| exposes_targets(state, ep_idx))
        .collect()
}

fn exposes_targets(state: &KeyspaceState, ep_idx: NodeIndex) -> Vec<NodeIndex> {
    state
        .graph
        .edges_directed(ep_idx, Direction::Outgoing)
        .filter(|e| e.weight().label.as_str() == EdgeLabel::EXPOSES)
        .map(|e| e.target())
        .collect()
}

// ---------------------------------------------------------------------------
// BFS + attribution
// ---------------------------------------------------------------------------

/// Per-seed BFS, accumulating `seed_idx → set_of_reached` into a single
/// `reach_count: NodeIndex → i64` map. Only `:Item` nodes are counted;
/// `:CallSite` nodes that the BFS transits through are filtered out at
/// attribution time. Every seed is self-reached (`+1` for its own entry).
fn accumulate_reach_counts(
    state: &KeyspaceState,
    seeds: &BTreeSet<NodeIndex>,
) -> BTreeMap<NodeIndex, i64> {
    let item_label = Label::new(Label::ITEM);
    seeds
        .iter()
        .flat_map(|&seed| bfs_call_graph(state, seed))
        .filter(|idx| is_label(state, *idx, &item_label))
        .fold(BTreeMap::new(), |mut acc, idx| {
            *acc.entry(idx).or_insert(0) += 1;
            acc
        })
}

/// BFS from `seed` via outgoing `CALLS` + `INVOKES_AT` edges. Follows both
/// the syn direct `(:Item)-[:CALLS]->(:Item)` shape and the HIR two-hop
/// `(:Item)-[:INVOKES_AT]->(:CallSite)-[:CALLS]->(:Item)` shape without
/// distinguishing them at walk time. The callsite intermediates are
/// filtered out at attribution.
fn bfs_call_graph(state: &KeyspaceState, seed: NodeIndex) -> BTreeSet<NodeIndex> {
    let mut visited: BTreeSet<NodeIndex> = BTreeSet::new();
    let mut queue: VecDeque<NodeIndex> = VecDeque::new();
    visited.insert(seed);
    queue.push_back(seed);
    while let Some(idx) = queue.pop_front() {
        for edge in state.graph.edges_directed(idx, Direction::Outgoing) {
            if is_call_graph_edge(edge.weight().label.as_str()) {
                let target = edge.target();
                if visited.insert(target) {
                    queue.push_back(target);
                }
            }
        }
    }
    visited
}

fn is_call_graph_edge(label: &str) -> bool {
    label == EdgeLabel::CALLS || label == EdgeLabel::INVOKES_AT
}

fn is_label(state: &KeyspaceState, idx: NodeIndex, label: &Label) -> bool {
    state
        .graph
        .node_weight(idx)
        .is_some_and(|n| n.label == *label)
}

// ---------------------------------------------------------------------------
// Attribute emission
// ---------------------------------------------------------------------------

/// For every `:Item` node, write `reachable_from_entry` (bool) and
/// `reachable_entry_count` (i64). Items not reached by any seed get
/// `(false, 0)` — explicit zero, never `Null`.
fn write_item_attrs(state: &mut KeyspaceState, reach_count: &BTreeMap<NodeIndex, i64>) -> u64 {
    let item_indices = state.nodes_with_label(&Label::new(Label::ITEM));
    let mut count: u64 = 0;
    for idx in item_indices {
        let reached = reach_count.get(&idx).copied().unwrap_or(0);
        if let Some(node) = state.graph.node_weight_mut(idx) {
            node.props
                .insert(ATTR_REACHABLE.into(), PropValue::Bool(reached > 0));
            node.props
                .insert(ATTR_COUNT.into(), PropValue::Int(reached));
            count += 2;
        }
    }
    count
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests;

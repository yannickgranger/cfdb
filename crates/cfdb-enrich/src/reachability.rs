//! `enrich_reachability` — BFS from every `:EntryPoint` over `CALLS*` edges,
//! writing `:Item.reachable_from_entry` (bool) + `:Item.reachable_entry_count`
//! (i64) per item.
//!
//! # Algorithm
//!
//! 1. **Seed set** — every `(:EntryPoint)-[:EXPOSES]->(:Item)` target is a
//!    handler item. Seeds are collected into a `BTreeSet<id>` for
//!    deterministic iteration.
//! 2. **Per-seed BFS** — for each seed, walk outgoing `CALLS` **and**
//!    `INVOKES_AT` edges until the frontier is exhausted. Both edge kinds
//!    are needed because the HIR extractor models dispatch as
//!    `(:Item)-[:INVOKES_AT]->(:CallSite)-[:CALLS]->(:Item)` (the two-hop
//!    path represents "this item invokes that callsite which resolves to
//!    that callee"); the syn-only path is `(:Item)-[:CALLS]->(:Item)`
//!    direct (no callsite intermediate). Walking both covers both shapes
//!    and lets the BFS traverse a mixed graph without distinguishing them.
//!    Nodes are interned to dense handles for the walk (see [`CallGraph`]);
//!    visited is a `BTreeSet<handle>`.
//! 3. **Attribution** — a `BTreeMap<id, i64>` counts how many distinct
//!    seeds reach each node. Only `:Item` nodes are attributed;
//!    transitively-visited `:CallSite` nodes are ignored at count time.
//! 4. **Write attrs** — every `:Item` node gets both attrs. Items with
//!    `count == 0` are explicitly marked `reachable_from_entry = false,
//!    reachable_entry_count = 0` — never silently left null.
//!
//! # Degraded path
//!
//! If the keyspace carries zero `:EntryPoint` nodes, the pass returns
//! `ran: false` with a clear warning naming `cfdb extract --features hir`.
//! **Never** silently mark every item unreachable in this case — the
//! classifier would misread that as "everything is unwired," which is
//! factually wrong (it just means the HIR pass that populates entry
//! points didn't run).
//!
//! # Determinism
//!
//! - Seed collection uses `BTreeSet<id>`.
//! - Per-seed BFS runs over a pass-local [`CallGraph`] that interns ids to
//!   dense handles and memoises each node's outgoing call edges, so a node
//!   is resolved through the port once no matter how many seeds reach it;
//!   visits go through `BTreeSet<handle>`.
//! - `reach_count` is a `BTreeMap<id, i64>`; per-seed visit order never
//!   influences it (pure count).
//! - Attribute writes iterate `nodes_with_label`, which per the port
//!   contract preserves the underlying storage's stable ordering (G1).
//!
//! Two runs on the same graph produce byte-identical canonical dumps.
//!
//! # Cycle safety
//!
//! BFS terminates because each visited node is recorded in the `BTreeSet`
//! before its outgoing edges are walked. A cycle `A → B → A` visits A
//! once, queues B, visits B, attempts to queue A (already visited, not
//! re-added), and the frontier drains.
//!
//! # Accuracy caveat
//!
//! `reachable_from_entry = false` is only as accurate as the `CALLS`
//! edges populated by `cfdb-hir-extractor`. The classifier applies
//! confidence gating on the "Unwired" class accordingly.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

use cfdb_core::enrich::EnrichReport;
use cfdb_core::fact::PropValue;
use cfdb_core::graph::GraphView;
use cfdb_core::schema::{Direction, EdgeLabel, Label};

pub(crate) const VERB: &str = "enrich_reachability";
/// `reachable_from_entry` attr name — shared with
/// [`crate::attr_call_resolution`] so the post-pass writes the same prop.
pub(crate) const ATTR_REACHABLE: &str = "reachable_from_entry";
const ATTR_COUNT: &str = "reachable_entry_count";
const ATTR_REACHABLE_PROD: &str = "reachable_from_production_entry";
const ATTR_COUNT_PROD: &str = "reachable_production_entry_count";

/// Selects which `:EntryPoint` kinds participate as BFS seeds.
///
/// - `All` — every entry point seeds the BFS (legacy behavior; writes
///   `reachable_from_entry` + `reachable_entry_count`).
/// - `ProductionOnly` — entry points with `kind ∈ {test, bench}` are
///   excluded, so the resulting reach set is the production-only call
///   closure (writes `reachable_from_production_entry` +
///   `reachable_production_entry_count`).
///
/// `pub(crate)` — never re-exported. The two-pass orchestration in
/// [`crate::EnrichEngine`] is the only caller outside this module.
#[derive(Clone, Copy)]
pub(crate) enum ReachabilityFilter {
    All,
    ProductionOnly,
}

impl ReachabilityFilter {
    fn reach_attr(self) -> &'static str {
        match self {
            Self::All => ATTR_REACHABLE,
            Self::ProductionOnly => ATTR_REACHABLE_PROD,
        }
    }

    fn count_attr(self) -> &'static str {
        match self {
            Self::All => ATTR_COUNT,
            Self::ProductionOnly => ATTR_COUNT_PROD,
        }
    }

    fn keep_entry_point(self, kind: Option<&str>) -> bool {
        match self {
            Self::All => true,
            Self::ProductionOnly => !matches!(kind, Some("test") | Some("bench")),
        }
    }
}

pub(crate) fn run(view: &mut dyn GraphView, filter: ReachabilityFilter) -> EnrichReport {
    let entry_points = view.nodes_with_label(&Label::new(Label::ENTRY_POINT));

    // Degraded path — refuse to mark every item `reachable_from_entry = false`
    // when there are no entry points at all. Check the unfiltered set: an
    // all-test catalog is NOT a degraded extract (HIR ran, entries exist,
    // they're just all test entries), so the ProductionOnly pass with an
    // all-test catalog still runs and writes `(false, 0)` for every item.
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

    let filtered = filter_entry_points(view, &entry_points, filter);
    let seeds = collect_seeds(view, &filtered);
    let reach_count = accumulate_reach_counts(view, &seeds);
    let bfs_attrs = write_item_attrs(view, &reach_count, filter);

    // Serde default callee post-pass. Marks functions referenced by
    // `#[serde(default = "fn")]` as reachable, since cfdb cannot trace through
    // proc-macro-expanded derive impls (see crate::attr_call_resolution).
    //
    // The post-pass writes to whichever reach attr the current filter
    // selected: `reachable_from_entry` for `All`, or
    // `reachable_from_production_entry` for `ProductionOnly`. Serde
    // deserialize callbacks are production code, so they belong in BOTH
    // sets — by running the post-pass once per filter invocation we
    // satisfy that without a separate dispatch.
    let attr_call_attrs = crate::attr_call_resolution::mark_serde_default_callees_reachable(
        view,
        filter.reach_attr(),
    );

    EnrichReport {
        verb: VERB.into(),
        ran: true,
        facts_scanned: u64::try_from(entry_points.len()).unwrap_or(u64::MAX),
        attrs_written: bfs_attrs + attr_call_attrs,
        edges_written: 0,
        warnings: Vec::new(),
    }
}

/// Filter the entry-point id set by the requested `ReachabilityFilter`,
/// reading each candidate's `kind` prop. Items with no `kind` prop fall
/// through as "keep" under both filters — the catalog is malformed but
/// the pass doesn't fail.
fn filter_entry_points(
    view: &dyn GraphView,
    entry_points: &[String],
    filter: ReachabilityFilter,
) -> Vec<String> {
    entry_points
        .iter()
        .filter(|id| {
            let kind = view
                .node_by_id(id)
                .and_then(|n| n.props.get("kind"))
                .and_then(PropValue::as_str);
            filter.keep_entry_point(kind)
        })
        .cloned()
        .collect()
}

// ---------------------------------------------------------------------------
// Seed collection
// ---------------------------------------------------------------------------

/// Collect the set of `:Item` ids that are EXPOSES-targets of some
/// `:EntryPoint`. An entry point with no outgoing EXPOSES edge contributes
/// no seed (the catalog is inconsistent, but we don't fail — the classifier
/// still gets useful data from the entry points that DO expose).
fn collect_seeds(view: &dyn GraphView, entry_points: &[String]) -> BTreeSet<String> {
    entry_points
        .iter()
        .flat_map(|ep_id| exposes_targets(view, ep_id))
        .collect()
}

fn exposes_targets(view: &dyn GraphView, ep_id: &str) -> Vec<String> {
    view.neighbors(ep_id, Direction::Out)
        .into_iter()
        .filter(|(label, _)| label.as_str() == EdgeLabel::EXPOSES)
        .map(|(_, target)| target)
        .collect()
}

// ---------------------------------------------------------------------------
// BFS + attribution
// ---------------------------------------------------------------------------

/// Pass-local, interned view of the call graph. Ids become dense `u32`
/// handles and each node's outgoing `CALLS`/`INVOKES_AT` targets are
/// resolved through the port exactly once, then reused by every seed's
/// BFS — the traversal itself never touches a `String`.
struct CallGraph<'v> {
    view: &'v dyn GraphView,
    ids: Vec<String>,
    handles: HashMap<String, u32>,
    successors: Vec<Option<Vec<u32>>>,
}

impl<'v> CallGraph<'v> {
    fn new(view: &'v dyn GraphView) -> Self {
        Self {
            view,
            ids: Vec::new(),
            handles: HashMap::new(),
            successors: Vec::new(),
        }
    }

    fn handle(&mut self, id: &str) -> u32 {
        if let Some(&h) = self.handles.get(id) {
            return h;
        }
        let h = u32::try_from(self.ids.len()).expect("node count fits u32");
        self.ids.push(id.to_string());
        self.handles.insert(id.to_string(), h);
        self.successors.push(None);
        h
    }

    fn id(&self, handle: u32) -> &str {
        &self.ids[handle as usize]
    }

    /// Outgoing call-graph targets of `handle`, resolved on first request.
    fn successors(&mut self, handle: u32) -> &[u32] {
        if self.successors[handle as usize].is_none() {
            let targets: Vec<u32> = self
                .view
                .neighbors(self.id(handle), Direction::Out)
                .into_iter()
                .filter(|(label, _)| is_call_graph_edge(label.as_str()))
                .map(|(_, target)| self.handle(&target))
                .collect();
            self.successors[handle as usize] = Some(targets);
        }
        self.successors[handle as usize]
            .as_deref()
            .expect("filled just above")
    }
}

/// Per-seed BFS, accumulating `seed → set_of_reached` into a single
/// `reach_count: id → i64` map. Only `:Item` nodes are counted;
/// `:CallSite` nodes that the BFS transits through are filtered out at
/// attribution time. Every seed is self-reached (`+1` for its own entry).
fn accumulate_reach_counts(
    view: &dyn GraphView,
    seeds: &BTreeSet<String>,
) -> BTreeMap<String, i64> {
    let item_label = Label::new(Label::ITEM);
    let mut graph = CallGraph::new(view);
    let mut counts: BTreeMap<u32, i64> = BTreeMap::new();
    for seed in seeds {
        let seed_handle = graph.handle(seed);
        for reached in bfs_call_graph(&mut graph, seed_handle) {
            *counts.entry(reached).or_insert(0) += 1;
        }
    }
    counts
        .into_iter()
        .filter(|(handle, _)| is_label(view, graph.id(*handle), &item_label))
        .map(|(handle, count)| (graph.id(handle).to_string(), count))
        .collect()
}

/// BFS from `seed` via outgoing `CALLS` + `INVOKES_AT` edges. Follows both
/// the syn direct `(:Item)-[:CALLS]->(:Item)` shape and the HIR two-hop
/// `(:Item)-[:INVOKES_AT]->(:CallSite)-[:CALLS]->(:Item)` shape without
/// distinguishing them at walk time. The callsite intermediates are
/// filtered out at attribution.
fn bfs_call_graph(graph: &mut CallGraph<'_>, seed: u32) -> BTreeSet<u32> {
    let mut visited: BTreeSet<u32> = BTreeSet::new();
    let mut queue: VecDeque<u32> = VecDeque::new();
    visited.insert(seed);
    queue.push_back(seed);
    while let Some(handle) = queue.pop_front() {
        for &target in graph.successors(handle) {
            if visited.insert(target) {
                queue.push_back(target);
            }
        }
    }
    visited
}

fn is_call_graph_edge(label: &str) -> bool {
    label == EdgeLabel::CALLS || label == EdgeLabel::INVOKES_AT
}

fn is_label(view: &dyn GraphView, id: &str, label: &Label) -> bool {
    view.node_by_id(id).is_some_and(|n| n.label == *label)
}

// ---------------------------------------------------------------------------
// Attribute emission
// ---------------------------------------------------------------------------

/// For every `:Item` node, write the reach/count pair selected by
/// `filter` — either `(reachable_from_entry, reachable_entry_count)` for
/// `All` or `(reachable_from_production_entry, reachable_production_entry_count)`
/// for `ProductionOnly`. Items not reached by any seed in the filtered
/// pass get `(false, 0)` — explicit zero, never `Null`.
fn write_item_attrs(
    view: &mut dyn GraphView,
    reach_count: &BTreeMap<String, i64>,
    filter: ReachabilityFilter,
) -> u64 {
    let item_ids = view.nodes_with_label(&Label::new(Label::ITEM));
    let reach_attr = filter.reach_attr();
    let count_attr = filter.count_attr();
    let mut count: u64 = 0;
    for id in item_ids {
        let reached = reach_count.get(&id).copied().unwrap_or(0);
        if view.set_attr(&id, reach_attr, PropValue::Bool(reached > 0))
            && view.set_attr(&id, count_attr, PropValue::Int(reached))
        {
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

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
mod tests {
    use cfdb_core::enrich::EnrichBackend;
    use cfdb_core::fact::{Edge, Node, PropValue, Props};
    use cfdb_core::schema::{EdgeLabel, Keyspace, Label};
    use cfdb_core::store::StoreBackend;

    use crate::PetgraphStore;

    /// Build an `:Item` node with given qname + crate. `id` = `item:{qname}`.
    fn item_node(qname: &str, crate_name: &str) -> Node {
        let mut props = Props::new();
        props.insert("qname".into(), PropValue::Str(qname.into()));
        props.insert("name".into(), PropValue::Str(qname.into()));
        props.insert("crate".into(), PropValue::Str(crate_name.into()));
        props.insert("file".into(), PropValue::Str("src/lib.rs".into()));
        Node {
            id: format!("item:{qname}"),
            label: Label::new(Label::ITEM),
            props,
        }
    }

    fn entry_point_node(id: &str) -> Node {
        let mut props = Props::new();
        props.insert("kind".into(), PropValue::Str("cli_command".into()));
        Node {
            id: id.into(),
            label: Label::new(Label::ENTRY_POINT),
            props,
        }
    }

    fn calls_edge(src: &str, dst: &str) -> Edge {
        Edge {
            src: src.into(),
            dst: dst.into(),
            label: EdgeLabel::new(EdgeLabel::CALLS),
            props: Props::new(),
        }
    }

    fn exposes_edge(src: &str, dst: &str) -> Edge {
        Edge {
            src: src.into(),
            dst: dst.into(),
            label: EdgeLabel::new(EdgeLabel::EXPOSES),
            props: Props::new(),
        }
    }

    fn get_item_props(store: &PetgraphStore, keyspace: &Keyspace, qname: &str) -> Props {
        let (nodes, _) = store.export(keyspace).expect("export");
        nodes
            .into_iter()
            .find(|n| {
                n.props
                    .get("qname")
                    .and_then(PropValue::as_str)
                    .is_some_and(|q| q == qname)
            })
            .unwrap_or_else(|| panic!(":Item {qname} not found"))
            .props
    }

    fn get_reachability(store: &PetgraphStore, keyspace: &Keyspace, qname: &str) -> (bool, i64) {
        let props = get_item_props(store, keyspace, qname);
        let r = props
            .get("reachable_from_entry")
            .and_then(|p| match p {
                PropValue::Bool(b) => Some(*b),
                _ => None,
            })
            .expect("reachable_from_entry must be Bool");
        let c = props
            .get("reachable_entry_count")
            .and_then(|p| match p {
                PropValue::Int(i) => Some(*i),
                _ => None,
            })
            .expect("reachable_entry_count must be Int");
        (r, c)
    }

    // ------------------------------------------------------------------
    // AC-1: 1 entry point E -[:EXPOSES]-> A; A -[:CALLS]-> B; C isolated.
    // ------------------------------------------------------------------

    #[test]
    fn ac1_three_item_fixture_reachability() {
        let mut store = PetgraphStore::new();
        let ks = Keyspace::new("test");
        store
            .ingest_nodes(
                &ks,
                vec![
                    entry_point_node("ep:E"),
                    item_node("A", "x"),
                    item_node("B", "x"),
                    item_node("C", "x"),
                ],
            )
            .expect("ingest nodes");
        store
            .ingest_edges(
                &ks,
                vec![
                    exposes_edge("ep:E", "item:A"),
                    calls_edge("item:A", "item:B"),
                ],
            )
            .expect("ingest edges");

        let report = store.enrich_reachability(&ks).expect("pass");

        assert!(report.ran);
        assert_eq!(report.facts_scanned, 1, "one entry point");
        assert_eq!(report.attrs_written, 6, "3 items × 2 attrs");

        assert_eq!(
            get_reachability(&store, &ks, "A"),
            (true, 1),
            "A self-seeded"
        );
        assert_eq!(
            get_reachability(&store, &ks, "B"),
            (true, 1),
            "B reachable via A"
        );
        assert_eq!(
            get_reachability(&store, &ks, "C"),
            (false, 0),
            "C unreachable from any entry point"
        );
    }

    // ------------------------------------------------------------------
    // AC-2: multi-entry-point attribution — two entry points reaching an
    // overlapping item should count 2.
    // ------------------------------------------------------------------

    #[test]
    fn ac2_multi_entry_attribution_counts_distinct_origins() {
        let mut store = PetgraphStore::new();
        let ks = Keyspace::new("test");
        // E1 -[:EXPOSES]-> A1 -[:CALLS]-> Shared
        // E2 -[:EXPOSES]-> A2 -[:CALLS]-> Shared
        // A1 and A2 are each reached by one entry point.
        // Shared is reached by both → count = 2.
        store
            .ingest_nodes(
                &ks,
                vec![
                    entry_point_node("ep:E1"),
                    entry_point_node("ep:E2"),
                    item_node("A1", "x"),
                    item_node("A2", "x"),
                    item_node("Shared", "x"),
                ],
            )
            .expect("ingest nodes");
        store
            .ingest_edges(
                &ks,
                vec![
                    exposes_edge("ep:E1", "item:A1"),
                    exposes_edge("ep:E2", "item:A2"),
                    calls_edge("item:A1", "item:Shared"),
                    calls_edge("item:A2", "item:Shared"),
                ],
            )
            .expect("ingest edges");

        store.enrich_reachability(&ks).expect("pass");

        assert_eq!(get_reachability(&store, &ks, "A1"), (true, 1));
        assert_eq!(get_reachability(&store, &ks, "A2"), (true, 1));
        assert_eq!(
            get_reachability(&store, &ks, "Shared"),
            (true, 2),
            "Shared reached by both E1 and E2"
        );
    }

    // ------------------------------------------------------------------
    // AC-3: zero :EntryPoint → ran=false + warning, no attrs touched.
    // ------------------------------------------------------------------

    #[test]
    fn ac3_zero_entry_points_returns_ran_false_with_warning() {
        let mut store = PetgraphStore::new();
        let ks = Keyspace::new("test");
        store
            .ingest_nodes(&ks, vec![item_node("A", "x"), item_node("B", "x")])
            .expect("ingest");
        // No :EntryPoint nodes ingested.

        let report = store.enrich_reachability(&ks).expect("pass");

        assert!(!report.ran, "zero entry points → ran=false");
        assert_eq!(report.attrs_written, 0, "no items touched");
        assert!(
            report.warnings.iter().any(|w| w.contains("features hir")),
            "warning must point at `cfdb extract --features hir`: {:?}",
            report.warnings
        );

        // Confirm no attrs were silently written on A or B.
        let props_a = get_item_props(&store, &ks, "A");
        assert!(!props_a.contains_key("reachable_from_entry"));
        assert!(!props_a.contains_key("reachable_entry_count"));
    }

    // ------------------------------------------------------------------
    // AC-5: cycle safety — graph with A -> B -> A terminates.
    // ------------------------------------------------------------------

    #[test]
    fn ac5_call_cycle_does_not_loop_forever() {
        let mut store = PetgraphStore::new();
        let ks = Keyspace::new("test");
        store
            .ingest_nodes(
                &ks,
                vec![
                    entry_point_node("ep:E"),
                    item_node("A", "x"),
                    item_node("B", "x"),
                ],
            )
            .expect("ingest");
        // Cycle: A → B → A. Both reachable from E.
        store
            .ingest_edges(
                &ks,
                vec![
                    exposes_edge("ep:E", "item:A"),
                    calls_edge("item:A", "item:B"),
                    calls_edge("item:B", "item:A"),
                ],
            )
            .expect("ingest");

        let report = store.enrich_reachability(&ks).expect("pass");

        assert!(report.ran);
        assert_eq!(get_reachability(&store, &ks, "A"), (true, 1));
        assert_eq!(get_reachability(&store, &ks, "B"), (true, 1));
    }

    // ------------------------------------------------------------------
    // AC-6: determinism across two runs.
    // ------------------------------------------------------------------

    #[test]
    fn ac6_two_runs_produce_identical_canonical_dumps() {
        fn build() -> PetgraphStore {
            let mut store = PetgraphStore::new();
            let ks = Keyspace::new("test");
            store
                .ingest_nodes(
                    &ks,
                    vec![
                        entry_point_node("ep:E1"),
                        entry_point_node("ep:E2"),
                        item_node("A", "x"),
                        item_node("B", "x"),
                        item_node("C", "x"),
                    ],
                )
                .expect("ingest");
            store
                .ingest_edges(
                    &ks,
                    vec![
                        exposes_edge("ep:E1", "item:A"),
                        exposes_edge("ep:E2", "item:B"),
                        calls_edge("item:A", "item:C"),
                        calls_edge("item:B", "item:C"),
                    ],
                )
                .expect("ingest");
            store
        }

        let ks = Keyspace::new("test");
        let mut s1 = build();
        s1.enrich_reachability(&ks).expect("run 1");
        let mut s2 = build();
        s2.enrich_reachability(&ks).expect("run 2");
        let d1 = s1.canonical_dump(&ks).expect("dump 1");
        let d2 = s2.canonical_dump(&ks).expect("dump 2");
        assert_eq!(d1, d2, "two runs must be byte-identical (AC-6)");
    }

    // ------------------------------------------------------------------
    // Entry-point item that EXPOSES nothing — contributes no seed. The
    // catalog is inconsistent (every :EntryPoint should EXPOSES an :Item),
    // but we don't fail the pass.
    // ------------------------------------------------------------------

    #[test]
    fn entry_point_without_exposes_is_ignored() {
        let mut store = PetgraphStore::new();
        let ks = Keyspace::new("test");
        store
            .ingest_nodes(
                &ks,
                vec![
                    entry_point_node("ep:E"), // no outgoing EXPOSES
                    item_node("A", "x"),
                ],
            )
            .expect("ingest");
        // No edges.

        let report = store.enrich_reachability(&ks).expect("pass");

        assert!(report.ran, "pass still runs when EP is dangling");
        assert_eq!(get_reachability(&store, &ks, "A"), (false, 0));
    }

    // ------------------------------------------------------------------
    // Other edge kinds (LABELED_AS, REFERENCED_BY) are NOT traversed.
    // ------------------------------------------------------------------

    #[test]
    fn bfs_ignores_non_calls_edges() {
        let mut store = PetgraphStore::new();
        let ks = Keyspace::new("test");
        store
            .ingest_nodes(
                &ks,
                vec![
                    entry_point_node("ep:E"),
                    item_node("A", "x"),
                    item_node("B", "x"),
                ],
            )
            .expect("ingest");
        store
            .ingest_edges(
                &ks,
                vec![
                    exposes_edge("ep:E", "item:A"),
                    // NOT a CALLS edge — BFS must not follow.
                    Edge {
                        src: "item:A".into(),
                        dst: "item:B".into(),
                        label: EdgeLabel::new(EdgeLabel::REFERENCED_BY),
                        props: Props::new(),
                    },
                ],
            )
            .expect("ingest");

        store.enrich_reachability(&ks).expect("pass");

        assert_eq!(get_reachability(&store, &ks, "A"), (true, 1));
        assert_eq!(
            get_reachability(&store, &ks, "B"),
            (false, 0),
            "B is reached only via REFERENCED_BY, which is NOT a call path"
        );
    }

    #[test]
    fn unknown_keyspace_returns_err() {
        let mut store = PetgraphStore::new();
        let ks = Keyspace::new("never");
        let err = store
            .enrich_reachability(&ks)
            .expect_err("unknown keyspace must err");
        assert!(format!("{err:?}").contains("UnknownKeyspace"));
    }
}

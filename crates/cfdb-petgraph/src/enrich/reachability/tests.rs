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

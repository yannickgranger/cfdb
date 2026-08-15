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
    entry_point_node_kind(id, "cli_command")
}

fn entry_point_node_kind(id: &str, kind: &str) -> Node {
    let mut props = Props::new();
    props.insert("kind".into(), PropValue::Str(kind.into()));
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
    read_reach_pair(
        store,
        keyspace,
        qname,
        "reachable_from_entry",
        "reachable_entry_count",
    )
}

fn get_production_reachability(
    store: &PetgraphStore,
    keyspace: &Keyspace,
    qname: &str,
) -> (bool, i64) {
    read_reach_pair(
        store,
        keyspace,
        qname,
        "reachable_from_production_entry",
        "reachable_production_entry_count",
    )
}

fn read_reach_pair(
    store: &PetgraphStore,
    keyspace: &Keyspace,
    qname: &str,
    reach_attr: &str,
    count_attr: &str,
) -> (bool, i64) {
    let props = get_item_props(store, keyspace, qname);
    let r = props
        .get(reach_attr)
        .and_then(|p| match p {
            PropValue::Bool(b) => Some(*b),
            _ => None,
        })
        .unwrap_or_else(|| panic!("{reach_attr} must be Bool on item {qname}"));
    let c = props
        .get(count_attr)
        .and_then(|p| match p {
            PropValue::Int(i) => Some(*i),
            _ => None,
        })
        .unwrap_or_else(|| panic!("{count_attr} must be Int on item {qname}"));
    (r, c)
}

// ------------------------------------------------------------------
// 1 entry point E -[:EXPOSES]-> A; A -[:CALLS]-> B; C isolated.
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
    assert_eq!(
        report.attrs_written, 12,
        "3 items × 4 attrs (All pass + ProductionOnly pass, RFC-042 §3.2)"
    );

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
// multi-entry-point attribution — two entry points reaching an
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
// zero :EntryPoint → ran=false + warning, no attrs touched.
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
// cycle safety — graph with A -> B -> A terminates.
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
// determinism across two runs.
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

// ==================================================================
// ReachabilityFilter::ProductionOnly
// ==================================================================
//
// Two EntryPoints — one
// kind=mcp_tool, one kind=test — each EXPOSES a distinct :Item. After
// `store.enrich_reachability(&ks)` the trait method runs BOTH passes
// (All then ProductionOnly). Pass 1 writes `reachable_from_entry`;
// Pass 2 writes `reachable_from_production_entry`. ProductionOnly
// excludes kind ∈ {test, bench} from the seed set.

#[test]
fn prod1_test_entry_excluded_from_production_pass() {
    let mut store = PetgraphStore::new();
    let ks = Keyspace::new("test");
    store
        .ingest_nodes(
            &ks,
            vec![
                entry_point_node_kind("ep:Mcp", "mcp_tool"),
                entry_point_node_kind("ep:Test", "test"),
                item_node("A", "x"),
                item_node("T", "x"),
            ],
        )
        .expect("ingest nodes");
    store
        .ingest_edges(
            &ks,
            vec![
                exposes_edge("ep:Mcp", "item:A"),
                exposes_edge("ep:Test", "item:T"),
            ],
        )
        .expect("ingest edges");

    let report = store.enrich_reachability(&ks).expect("pass");

    assert!(report.ran);
    assert_eq!(
        report.attrs_written, 8,
        "2 items × 4 attrs — both passes (All + ProductionOnly) write 2 each"
    );

    // Pass 1 (All) — both items reachable.
    assert_eq!(get_reachability(&store, &ks, "A"), (true, 1));
    assert_eq!(get_reachability(&store, &ks, "T"), (true, 1));

    // Pass 2 (ProductionOnly) — only the mcp_tool-exposed item.
    assert_eq!(get_production_reachability(&store, &ks, "A"), (true, 1));
    assert_eq!(
        get_production_reachability(&store, &ks, "T"),
        (false, 0),
        "T is only reached by kind=test entry, excluded from ProductionOnly"
    );
}

#[test]
fn prod2_bench_entry_excluded_from_production_pass() {
    let mut store = PetgraphStore::new();
    let ks = Keyspace::new("test");
    store
        .ingest_nodes(
            &ks,
            vec![
                entry_point_node_kind("ep:Cli", "cli_command"),
                entry_point_node_kind("ep:Bench", "bench"),
                item_node("A", "x"),
                item_node("B", "x"),
            ],
        )
        .expect("ingest");
    store
        .ingest_edges(
            &ks,
            vec![
                exposes_edge("ep:Cli", "item:A"),
                exposes_edge("ep:Bench", "item:B"),
            ],
        )
        .expect("ingest");

    store.enrich_reachability(&ks).expect("pass");

    assert_eq!(get_reachability(&store, &ks, "A"), (true, 1));
    assert_eq!(get_reachability(&store, &ks, "B"), (true, 1));
    assert_eq!(get_production_reachability(&store, &ks, "A"), (true, 1));
    assert_eq!(
        get_production_reachability(&store, &ks, "B"),
        (false, 0),
        "B is only reached by kind=bench entry, excluded from ProductionOnly"
    );
}

#[test]
fn prod3_shared_item_counts_only_production_entries_in_prod_pass() {
    // ep:Http -[:EXPOSES]-> A1 -[:CALLS]-> Shared
    // ep:Test -[:EXPOSES]-> T1 -[:CALLS]-> Shared
    // All-pass: Shared reached from seeds {A1, T1} → count=2.
    // ProductionOnly: ep:Test filtered out, seeds {A1} → count=1.
    let mut store = PetgraphStore::new();
    let ks = Keyspace::new("test");
    store
        .ingest_nodes(
            &ks,
            vec![
                entry_point_node_kind("ep:Http", "http_route"),
                entry_point_node_kind("ep:Test", "test"),
                item_node("A1", "x"),
                item_node("T1", "x"),
                item_node("Shared", "x"),
            ],
        )
        .expect("ingest");
    store
        .ingest_edges(
            &ks,
            vec![
                exposes_edge("ep:Http", "item:A1"),
                exposes_edge("ep:Test", "item:T1"),
                calls_edge("item:A1", "item:Shared"),
                calls_edge("item:T1", "item:Shared"),
            ],
        )
        .expect("ingest");

    store.enrich_reachability(&ks).expect("pass");

    assert_eq!(
        get_reachability(&store, &ks, "Shared"),
        (true, 2),
        "All-pass: Shared reached from both A1 and T1 seeds"
    );
    assert_eq!(
        get_production_reachability(&store, &ks, "Shared"),
        (true, 1),
        "Production pass: only A1 is a seed (T1's EP filtered out)"
    );
    // T1 itself: All-pass true (seeded by its test EP); Prod-pass false
    // (T1's EP filtered out, no other seed reaches it).
    assert_eq!(get_reachability(&store, &ks, "T1"), (true, 1));
    assert_eq!(get_production_reachability(&store, &ks, "T1"), (false, 0));
}

#[test]
fn prod4_all_test_entries_writes_explicit_false_for_all_items() {
    // Catalog has entry points, but ALL of them are kind=test. ProductionOnly
    // pass produces zero seeds → every :Item gets (false, 0). The pass still
    // ran=true (degraded-path warning only fires on zero unfiltered EPs).
    let mut store = PetgraphStore::new();
    let ks = Keyspace::new("test");
    store
        .ingest_nodes(
            &ks,
            vec![
                entry_point_node_kind("ep:T1", "test"),
                entry_point_node_kind("ep:T2", "test"),
                item_node("A", "x"),
                item_node("B", "x"),
            ],
        )
        .expect("ingest");
    store
        .ingest_edges(
            &ks,
            vec![
                exposes_edge("ep:T1", "item:A"),
                exposes_edge("ep:T2", "item:B"),
            ],
        )
        .expect("ingest");

    let report = store.enrich_reachability(&ks).expect("pass");

    assert!(
        report.ran,
        "pass still runs — unfiltered EP set is non-empty"
    );
    // Both items have All=true (reached by their test EP) but Prod=false
    // (every EP filtered out).
    assert_eq!(get_reachability(&store, &ks, "A"), (true, 1));
    assert_eq!(get_reachability(&store, &ks, "B"), (true, 1));
    assert_eq!(
        get_production_reachability(&store, &ks, "A"),
        (false, 0),
        "all-test-EP catalog must mark items unreached-from-production explicitly"
    );
    assert_eq!(get_production_reachability(&store, &ks, "B"), (false, 0));
}

#[test]
fn prod5_two_runs_with_mixed_kinds_are_byte_identical() {
    fn build() -> PetgraphStore {
        let mut store = PetgraphStore::new();
        let ks = Keyspace::new("test");
        store
            .ingest_nodes(
                &ks,
                vec![
                    entry_point_node_kind("ep:Mcp", "mcp_tool"),
                    entry_point_node_kind("ep:Test", "test"),
                    entry_point_node_kind("ep:Bench", "bench"),
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
                    exposes_edge("ep:Mcp", "item:A"),
                    exposes_edge("ep:Test", "item:B"),
                    exposes_edge("ep:Bench", "item:C"),
                    calls_edge("item:A", "item:C"),
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
    assert_eq!(
        d1, d2,
        "dual-pass writes (All + ProductionOnly) must be byte-identical across runs"
    );
}

// ==================================================================
// serde_default callee resolution post-pass
// ==================================================================
//
// `:CallSite{kind="serde_default"}` carries a `callee_path` string
// referencing a fn invoked by serde's derived `Deserialize` impl. The
// derive expansion is invisible to cfdb (proc-macro server is disabled),
// so the BFS never reaches the callee through a normal call
// chain. Without the post-pass, every `#[serde(default = "fn")]`
// callee is flagged `unwired`.
//
// The post-pass walks every `:CallSite{kind="serde_default"}` node,
// resolves `callee_path` against `:Item.qname` using
// exact / same-module / same-crate candidate matching, and sets
// `reachable_from_entry = true` (and, on the ProductionOnly pass,
// `reachable_from_production_entry = true`) on the resolved item.
// `reachable_entry_count` is intentionally NOT incremented — the count
// semantic remains "distinct BFS-reaching entry points".

fn serde_default_callsite(parent_qname: &str, field: &str, callee_path: &str) -> Node {
    let mut props = Props::new();
    props.insert("kind".into(), PropValue::Str("serde_default".into()));
    props.insert("callee_path".into(), PropValue::Str(callee_path.into()));
    props.insert("caller_qname".into(), PropValue::Str(parent_qname.into()));
    props.insert("field".into(), PropValue::Str(field.into()));
    Node {
        id: format!("callsite:{parent_qname}.{field}:{callee_path}:0"),
        label: Label::new(Label::CALL_SITE),
        props,
    }
}

fn invokes_at_edge(src: &str, dst: &str) -> Edge {
    Edge {
        src: src.into(),
        dst: dst.into(),
        label: EdgeLabel::new(EdgeLabel::INVOKES_AT),
        props: Props::new(),
    }
}

#[test]
fn issue_396_serde_default_callee_with_exact_qname_match_becomes_reachable() {
    let mut store = PetgraphStore::new();
    let ks = Keyspace::new("test");
    // Entry point E → load_config (reachable via BFS).
    // AppConfig is a struct (NOT reached by BFS — types aren't called).
    // AppConfig has #[serde(default = "myapp::config::default_url")] on
    // the `url` field; callee_path is the FULLY-QUALIFIED form (the
    // exact-match resolution strategy must catch this case).
    // default_url itself has NO CALLS predecessor.
    store
        .ingest_nodes(
            &ks,
            vec![
                entry_point_node("ep:E"),
                item_node("myapp::load_config", "myapp"),
                item_node("myapp::config::AppConfig", "myapp"),
                item_node("myapp::config::default_url", "myapp"),
                serde_default_callsite(
                    "myapp::config::AppConfig",
                    "url",
                    "myapp::config::default_url",
                ),
            ],
        )
        .expect("ingest nodes");
    store
        .ingest_edges(
            &ks,
            vec![
                exposes_edge("ep:E", "item:myapp::load_config"),
                invokes_at_edge(
                    "item:myapp::config::AppConfig",
                    "callsite:myapp::config::AppConfig.url:myapp::config::default_url:0",
                ),
            ],
        )
        .expect("ingest edges");

    store.enrich_reachability(&ks).expect("pass");

    assert_eq!(
        get_reachability(&store, &ks, "myapp::load_config"),
        (true, 1),
        "load_config reached by BFS from E"
    );
    assert_eq!(
        get_reachability(&store, &ks, "myapp::config::AppConfig"),
        (false, 0),
        "AppConfig is a type, not in the call graph"
    );
    let (reachable, count) = get_reachability(&store, &ks, "myapp::config::default_url");
    assert!(
        reachable,
        "default_url MUST be reachable via the serde_default post-pass (#396) — \
         callee_path = exact qname match against :Item"
    );
    assert_eq!(
        count, 0,
        "reachable_entry_count stays at 0 — the post-pass marks the fn reachable \
         without attributing it to any specific entry point"
    );
}

#[test]
fn issue_396_serde_default_callee_with_same_module_short_form_becomes_reachable() {
    let mut store = PetgraphStore::new();
    let ks = Keyspace::new("test");
    // Author wrote #[serde(default = "default_url")] — the short form
    // that assumes default_url is in the SAME MODULE as AppConfig.
    // The same-module resolver must split AppConfig's qname on the
    // last `::` to recover the module path, then prepend it to
    // callee_path.
    store
        .ingest_nodes(
            &ks,
            vec![
                entry_point_node("ep:E"),
                item_node("myapp::load_config", "myapp"),
                item_node("myapp::config::AppConfig", "myapp"),
                item_node("myapp::config::default_url", "myapp"),
                serde_default_callsite("myapp::config::AppConfig", "url", "default_url"),
            ],
        )
        .expect("ingest nodes");
    store
        .ingest_edges(
            &ks,
            vec![
                exposes_edge("ep:E", "item:myapp::load_config"),
                invokes_at_edge(
                    "item:myapp::config::AppConfig",
                    "callsite:myapp::config::AppConfig.url:default_url:0",
                ),
            ],
        )
        .expect("ingest edges");

    store.enrich_reachability(&ks).expect("pass");

    let (reachable, _) = get_reachability(&store, &ks, "myapp::config::default_url");
    assert!(
        reachable,
        "default_url MUST be reachable via the serde_default post-pass (#396) — \
         same-module short-form resolution"
    );
}

#[test]
fn issue_396_serde_default_callee_unresolvable_does_not_panic() {
    let mut store = PetgraphStore::new();
    let ks = Keyspace::new("test");
    // callee_path resolves to NO :Item — e.g. the callee is in a
    // dependency crate not included in the workspace. The post-pass
    // must skip silently, not panic.
    store
        .ingest_nodes(
            &ks,
            vec![
                entry_point_node("ep:E"),
                item_node("myapp::load_config", "myapp"),
                item_node("myapp::config::AppConfig", "myapp"),
                serde_default_callsite("myapp::config::AppConfig", "timestamp", "chrono::Utc::now"),
            ],
        )
        .expect("ingest");
    store
        .ingest_edges(
            &ks,
            vec![
                exposes_edge("ep:E", "item:myapp::load_config"),
                invokes_at_edge(
                    "item:myapp::config::AppConfig",
                    "callsite:myapp::config::AppConfig.timestamp:chrono::Utc::now:0",
                ),
            ],
        )
        .expect("ingest");

    // Must not panic. AppConfig stays unreachable (no caller).
    store.enrich_reachability(&ks).expect("pass");
    assert_eq!(
        get_reachability(&store, &ks, "myapp::config::AppConfig"),
        (false, 0)
    );
}

#[test]
fn issue_396_non_serde_default_callsite_is_ignored() {
    let mut store = PetgraphStore::new();
    let ks = Keyspace::new("test");
    // A regular `:CallSite{kind="call"}` (from a normal fn body) must
    // NOT be picked up by the serde_default post-pass — it would
    // contaminate reachability with un-resolved textual callee paths.
    let mut cs_props = Props::new();
    cs_props.insert("kind".into(), PropValue::Str("call".into()));
    cs_props.insert(
        "callee_path".into(),
        PropValue::Str("myapp::dead_fn".into()),
    );
    cs_props.insert(
        "caller_qname".into(),
        PropValue::Str("myapp::load_config".into()),
    );
    store
        .ingest_nodes(
            &ks,
            vec![
                entry_point_node("ep:E"),
                item_node("myapp::load_config", "myapp"),
                item_node("myapp::dead_fn", "myapp"),
                Node {
                    id: "callsite:myapp::load_config:myapp::dead_fn:0".into(),
                    label: Label::new(Label::CALL_SITE),
                    props: cs_props,
                },
            ],
        )
        .expect("ingest");
    store
        .ingest_edges(&ks, vec![exposes_edge("ep:E", "item:myapp::load_config")])
        .expect("ingest");

    store.enrich_reachability(&ks).expect("pass");

    // dead_fn must stay unreachable — its callsite kind is "call",
    // not "serde_default", so the post-pass must not touch it.
    assert_eq!(
        get_reachability(&store, &ks, "myapp::dead_fn"),
        (false, 0),
        "post-pass must scope its writes to kind=serde_default callsites"
    );
}

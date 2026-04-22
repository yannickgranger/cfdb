//! Pattern B vertical-split-brain scar tests (issue #44).
//!
//! Asserts that `examples/queries/vertical-split-brain.cypher` fires on
//! the ground-truth shapes enumerated in
//! `docs/RFC-cfdb.md` §A1.3 — the resolver-fork
//! failure mode observed in qbot-core #2651, #3522, #3545, #3654.
//!
//! ## Test approach — direct fact injection
//!
//! Each scar test builds a synthetic `(Vec<Node>, Vec<Edge>)` batch
//! mirroring what the HIR extractor would emit for the corresponding
//! Rust fixture under `examples/queries/fixtures/vertical-split-brain/`,
//! ingests it into a fresh `PetgraphStore`, parses the query file, and
//! runs it. This is the same pattern as `cfdb-petgraph/src/tests.rs`'s
//! F1b / F2 study checks: we care that the rule SHAPE fires on the
//! expected graph shape, not that the extractor is correct end-to-end.
//! Extractor correctness is covered by
//! `cfdb-hir-extractor/tests/entry_point.rs` (entry point emission) and
//! `cfdb-hir-petgraph-adapter/tests/cfdb_self_dogfood.rs` (CALLS edges
//! end-to-end). Pattern B's concern is the query, not the extraction.
//!
//! Direct injection is faster (sub-second per test vs tens of seconds
//! for HIR lowering) and isolates the failure signal: a Pattern B
//! regression here cannot be blamed on HIR instability.
//!
//! ## The fixture in the repo (`examples/queries/fixtures/vsb/*`)
//!
//! The on-disk Rust fixture exists for two reasons: (1) a human can
//! run `cfdb extract --workspace examples/queries/fixtures/vertical-split-brain`
//! locally and confirm the cypher rule fires; (2) a future v0.3
//! scar test can lower the fixture through HIR end-to-end as soon as
//! resolution reaches the 80% recall target of RFC-032. Until then the
//! direct-injection assertions are the regression surface.

use std::collections::BTreeMap;
use std::path::PathBuf;

use cfdb_core::fact::{Edge, Node, PropValue};
use cfdb_core::qname::item_node_id;
use cfdb_core::schema::{EdgeLabel, Keyspace, Label};
use cfdb_core::store::StoreBackend;
use cfdb_petgraph::PetgraphStore;
use cfdb_query::parse;

// ---------------------------------------------------------------------
// Helpers — small, test-local builders for the fact shapes the HIR
// extractor would emit.
// ---------------------------------------------------------------------

fn entry_point_node(display_name: &str, handler_qname: &str, kind: &str) -> Node {
    let mut props = BTreeMap::new();
    props.insert("name".into(), PropValue::Str(display_name.into()));
    props.insert("kind".into(), PropValue::Str(kind.into()));
    props.insert("handler_qname".into(), PropValue::Str(handler_qname.into()));
    props.insert("file".into(), PropValue::Str("synthetic".into()));
    props.insert("params".into(), PropValue::Str("[]".into()));
    Node {
        id: format!("entrypoint:{kind}:{handler_qname}"),
        label: Label::new(Label::ENTRY_POINT),
        props,
    }
}

fn fn_item_node(qname: &str, name: &str) -> Node {
    let mut props = BTreeMap::new();
    props.insert("qname".into(), PropValue::Str(qname.into()));
    props.insert("name".into(), PropValue::Str(name.into()));
    props.insert("kind".into(), PropValue::Str("fn".into()));
    props.insert("crate".into(), PropValue::Str("vsb_fixture".into()));
    props.insert("file".into(), PropValue::Str("synthetic".into()));
    props.insert("line".into(), PropValue::Int(0));
    props.insert("is_test".into(), PropValue::Bool(false));
    Node {
        id: item_node_id(qname),
        label: Label::new(Label::ITEM),
        props,
    }
}

fn struct_item_node(qname: &str, name: &str) -> Node {
    let mut props = BTreeMap::new();
    props.insert("qname".into(), PropValue::Str(qname.into()));
    props.insert("name".into(), PropValue::Str(name.into()));
    props.insert("kind".into(), PropValue::Str("struct".into()));
    props.insert("crate".into(), PropValue::Str("vsb_fixture".into()));
    props.insert("file".into(), PropValue::Str("synthetic".into()));
    props.insert("line".into(), PropValue::Int(0));
    props.insert("is_test".into(), PropValue::Bool(false));
    Node {
        id: item_node_id(qname),
        label: Label::new(Label::ITEM),
        props,
    }
}

fn exposes_edge(ep_id: &str, handler_qname: &str) -> Edge {
    Edge {
        src: ep_id.into(),
        dst: item_node_id(handler_qname),
        label: EdgeLabel::new(EdgeLabel::EXPOSES),
        props: BTreeMap::new(),
    }
}

fn calls_edge(caller_qname: &str, callee_qname: &str) -> Edge {
    let mut props = BTreeMap::new();
    props.insert("resolved".into(), PropValue::Bool(true));
    Edge {
        src: item_node_id(caller_qname),
        dst: item_node_id(callee_qname),
        label: EdgeLabel::new(EdgeLabel::CALLS),
        props,
    }
}

fn keyspace() -> Keyspace {
    Keyspace::new("vsb-test")
}

/// Resolve the cfdb workspace root — two parents up from this crate's
/// manifest dir (same formula as `cfdb-recall/tests/integration_recall`).
fn workspace_root() -> PathBuf {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    PathBuf::from(manifest_dir)
        .parent()
        .and_then(|p| p.parent())
        .expect("cfdb workspace root — two parents up from cfdb-petgraph/")
        .to_path_buf()
}

fn load_query_text() -> String {
    let path = workspace_root().join("examples/queries/vertical-split-brain.cypher");
    std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "read vertical-split-brain.cypher at {}: {e}",
            path.display()
        )
    })
}

fn run_query(store: &PetgraphStore) -> Vec<BTreeMap<String, cfdb_core::result::RowValue>> {
    let text = load_query_text();
    let query = parse(&text).unwrap_or_else(|e| panic!("parse vertical-split-brain.cypher: {e:?}"));
    let result = store
        .execute(&keyspace(), &query)
        .expect("execute vertical-split-brain on fresh store");
    result.rows
}

fn row_str<'a>(
    row: &'a BTreeMap<String, cfdb_core::result::RowValue>,
    key: &str,
) -> Option<&'a str> {
    row.get(key).and_then(|v| v.as_str())
}

// ---------------------------------------------------------------------
// Scar tests — one per motivating qbot-core issue.
// ---------------------------------------------------------------------

/// Scar for qbot-core #2651 — compound-stop param drop.
///
/// Shape: one CLI entry point, one engine method, two `StopLoss`
/// resolvers (`from_bps`, `from_pct`) both reachable. Expected result:
/// exactly one row with `divergence_kind = "fork"`.
#[test]
fn scar_2651_compound_stop_emits_one_fork_row() {
    let mut store = PetgraphStore::new();
    let ks = keyspace();

    let cli_qname = "vsb_fixture::Cli";
    let handle_qname = "vsb_fixture::Cli::handle";
    let build_stop_qname = "vsb_fixture::Engine::build_stop";
    let from_bps_qname = "vsb_fixture::StopLoss::stop_from_bps";
    let from_pct_qname = "vsb_fixture::StopLoss::stop_from_pct";

    let nodes = vec![
        entry_point_node("Cli", cli_qname, "cli_command"),
        struct_item_node(cli_qname, "Cli"),
        fn_item_node(handle_qname, "handle"),
        fn_item_node(build_stop_qname, "build_stop"),
        fn_item_node(from_bps_qname, "stop_from_bps"),
        fn_item_node(from_pct_qname, "stop_from_pct"),
    ];
    let edges = vec![
        exposes_edge(&format!("entrypoint:cli_command:{cli_qname}"), cli_qname),
        // CALLS chain: Cli -> Cli::handle -> Engine::build_stop ->
        // {StopLoss::from_bps, StopLoss::from_pct}.
        calls_edge(cli_qname, handle_qname),
        calls_edge(handle_qname, build_stop_qname),
        calls_edge(build_stop_qname, from_bps_qname),
        calls_edge(build_stop_qname, from_pct_qname),
    ];

    store
        .ingest_nodes(&ks, nodes)
        .expect("ingest synthetic nodes");
    store
        .ingest_edges(&ks, edges)
        .expect("ingest synthetic edges");

    let rows = run_query(&store);

    assert_eq!(
        rows.len(),
        1,
        "expected exactly 1 Pattern B fork row for #2651; got {}: {:?}",
        rows.len(),
        rows
    );

    let row = &rows[0];
    assert_eq!(row_str(row, "entry_point"), Some("Cli"));
    assert_eq!(row_str(row, "entry_qname"), Some(cli_qname));
    assert_eq!(row_str(row, "resolver_a_qname"), Some(from_bps_qname));
    assert_eq!(row_str(row, "resolver_b_qname"), Some(from_pct_qname));
    assert_eq!(row_str(row, "divergence_kind"), Some("fork"));
    // `regexp_extract(name, '^(\w+)_(?:from|to|for|as)_')` returns
    // the substring up to and including the first keyword boundary.
    // For `stop_from_bps` that is `stop_from_`; both resolvers agree.
    assert_eq!(row_str(row, "concept_prefix"), Some("stop_from_"));
}

/// Scar for qbot-core #3522 — pair-resolution split.
///
/// Two distinct resolver names with a non-trivial shared concept
/// prefix: `pair_from_alias` vs `pair_from_symbol`. Expected:
/// one row; `concept_prefix` deterministically matches `pair_from_`.
#[test]
fn scar_3522_pair_resolution_reports_concept_prefix() {
    let mut store = PetgraphStore::new();
    let ks = keyspace();

    let cli_qname = "vsb_fixture::PairCli";
    let handle_qname = "vsb_fixture::PairCli::handle";
    let resolver_alias = "vsb_fixture::Pair::pair_from_alias";
    let resolver_symbol = "vsb_fixture::Pair::pair_from_symbol";

    let nodes = vec![
        entry_point_node("PairCli", cli_qname, "cli_command"),
        struct_item_node(cli_qname, "PairCli"),
        fn_item_node(handle_qname, "handle"),
        fn_item_node(resolver_alias, "pair_from_alias"),
        fn_item_node(resolver_symbol, "pair_from_symbol"),
    ];
    let edges = vec![
        exposes_edge(&format!("entrypoint:cli_command:{cli_qname}"), cli_qname),
        calls_edge(cli_qname, handle_qname),
        calls_edge(handle_qname, resolver_alias),
        calls_edge(handle_qname, resolver_symbol),
    ];

    store.ingest_nodes(&ks, nodes).expect("ingest nodes");
    store.ingest_edges(&ks, edges).expect("ingest edges");

    let rows = run_query(&store);
    assert_eq!(
        rows.len(),
        1,
        "expected exactly 1 fork row for #3522; got {rows:?}"
    );
    let row = &rows[0];
    assert_eq!(row_str(row, "resolver_a_qname"), Some(resolver_alias));
    assert_eq!(row_str(row, "resolver_b_qname"), Some(resolver_symbol));
    assert_eq!(
        row_str(row, "concept_prefix"),
        Some("pair_from_"),
        "concept_prefix should be the shared `pair_from_` segment"
    );
    assert_eq!(row_str(row, "divergence_kind"), Some("fork"));
}

/// Scar for qbot-core #3545 — `build_resolved_config` 3-way scatter.
///
/// Three reachable resolvers sharing a `config_from_` prefix. The rule
/// emits ALL pairs `(a.qname < b.qname)` — for 3 resolvers that's
/// C(3,2) = 3 rows. This proves the rule scales with the scatter
/// fan-out and does not collapse to a single "any split-brain" row.
#[test]
fn scar_3545_three_way_scatter_emits_three_pairs() {
    let mut store = PetgraphStore::new();
    let ks = keyspace();

    let cli_qname = "vsb_fixture::ConfigCli";
    let handle_qname = "vsb_fixture::ConfigCli::handle";
    let r_env = "vsb_fixture::Config::config_from_env";
    let r_file = "vsb_fixture::Config::config_from_file";
    let r_flags = "vsb_fixture::Config::config_from_flags";

    let nodes = vec![
        entry_point_node("ConfigCli", cli_qname, "cli_command"),
        struct_item_node(cli_qname, "ConfigCli"),
        fn_item_node(handle_qname, "handle"),
        fn_item_node(r_env, "config_from_env"),
        fn_item_node(r_file, "config_from_file"),
        fn_item_node(r_flags, "config_from_flags"),
    ];
    let edges = vec![
        exposes_edge(&format!("entrypoint:cli_command:{cli_qname}"), cli_qname),
        calls_edge(cli_qname, handle_qname),
        calls_edge(handle_qname, r_env),
        calls_edge(handle_qname, r_file),
        calls_edge(handle_qname, r_flags),
    ];

    store.ingest_nodes(&ks, nodes).expect("ingest nodes");
    store.ingest_edges(&ks, edges).expect("ingest edges");

    let rows = run_query(&store);
    assert_eq!(
        rows.len(),
        3,
        "C(3,2)=3 pairs expected for #3545 three-way scatter; got {rows:?}"
    );

    // All three rows must agree on the concept prefix.
    for row in &rows {
        assert_eq!(
            row_str(row, "concept_prefix"),
            Some("config_from_"),
            "every row must share the `config_from_` prefix; row={row:?}"
        );
        assert_eq!(row_str(row, "divergence_kind"), Some("fork"));
    }

    // Deterministic ordering invariant: qname a < qname b, and rows
    // ordered by (entry_point, concept_prefix, resolver_a_qname).
    let a_qnames: Vec<&str> = rows
        .iter()
        .map(|r| row_str(r, "resolver_a_qname").unwrap())
        .collect();
    let mut sorted = a_qnames.clone();
    sorted.sort();
    assert_eq!(a_qnames, sorted, "rows must be sorted by resolver_a_qname");
}

/// Scar for qbot-core #3654 — seven split resolution points, same
/// concept. Expected pair count is C(7,2) = 21. This is the
/// upper-bound stress case on the RFC-cited backlog.
#[test]
fn scar_3654_seven_way_scatter_emits_twenty_one_pairs() {
    let mut store = PetgraphStore::new();
    let ks = keyspace();

    let cli_qname = "vsb_fixture::BigCli";
    let handle_qname = "vsb_fixture::BigCli::handle";
    let suffixes = [
        "alpha", "bravo", "charlie", "delta", "echo", "foxtrot", "golf",
    ];

    let mut nodes = vec![
        entry_point_node("BigCli", cli_qname, "cli_command"),
        struct_item_node(cli_qname, "BigCli"),
        fn_item_node(handle_qname, "handle"),
    ];
    let mut edges = vec![
        exposes_edge(&format!("entrypoint:cli_command:{cli_qname}"), cli_qname),
        calls_edge(cli_qname, handle_qname),
    ];
    for suffix in &suffixes {
        let name = format!("sizing_from_{suffix}");
        let qname = format!("vsb_fixture::Sizing::{name}");
        nodes.push(fn_item_node(&qname, &name));
        edges.push(calls_edge(handle_qname, &qname));
    }

    store.ingest_nodes(&ks, nodes).expect("ingest nodes");
    store.ingest_edges(&ks, edges).expect("ingest edges");

    let rows = run_query(&store);
    assert_eq!(
        rows.len(),
        21,
        "C(7,2)=21 pairs expected for #3654 seven-way scatter; got {}",
        rows.len()
    );
    for row in &rows {
        assert_eq!(row_str(row, "concept_prefix"), Some("sizing_from_"));
        assert_eq!(row_str(row, "divergence_kind"), Some("fork"));
    }
}

// ---------------------------------------------------------------------
// Negative tests — shapes the rule MUST NOT fire on.
// ---------------------------------------------------------------------

/// A single resolver reachable from the entry point is not split-brain
/// and must not emit any row.
#[test]
fn single_resolver_emits_no_rows() {
    let mut store = PetgraphStore::new();
    let ks = keyspace();

    let cli_qname = "vsb_fixture::LoneCli";
    let resolver = "vsb_fixture::Stop::stop_from_bps";

    let nodes = vec![
        entry_point_node("LoneCli", cli_qname, "cli_command"),
        struct_item_node(cli_qname, "LoneCli"),
        fn_item_node(resolver, "stop_from_bps"),
    ];
    let edges = vec![
        exposes_edge(&format!("entrypoint:cli_command:{cli_qname}"), cli_qname),
        calls_edge(cli_qname, resolver),
    ];

    store.ingest_nodes(&ks, nodes).expect("ingest nodes");
    store.ingest_edges(&ks, edges).expect("ingest edges");

    let rows = run_query(&store);
    assert!(
        rows.is_empty(),
        "single resolver must not be reported as fork; got {rows:?}"
    );
}

/// Two resolvers reachable from DIFFERENT entry points are two separate
/// resolution paths, not a fork inside one path. The rule's join on a
/// single entry point must exclude this shape.
#[test]
fn resolvers_under_distinct_entry_points_emit_no_rows() {
    let mut store = PetgraphStore::new();
    let ks = keyspace();

    let cli_a_qname = "vsb_fixture::CliA";
    let cli_b_qname = "vsb_fixture::CliB";
    let r_a = "vsb_fixture::Stop::stop_from_bps";
    let r_b = "vsb_fixture::Stop::stop_from_pct";

    let nodes = vec![
        entry_point_node("CliA", cli_a_qname, "cli_command"),
        entry_point_node("CliB", cli_b_qname, "cli_command"),
        struct_item_node(cli_a_qname, "CliA"),
        struct_item_node(cli_b_qname, "CliB"),
        fn_item_node(r_a, "stop_from_bps"),
        fn_item_node(r_b, "stop_from_pct"),
    ];
    let edges = vec![
        exposes_edge(
            &format!("entrypoint:cli_command:{cli_a_qname}"),
            cli_a_qname,
        ),
        exposes_edge(
            &format!("entrypoint:cli_command:{cli_b_qname}"),
            cli_b_qname,
        ),
        calls_edge(cli_a_qname, r_a),
        calls_edge(cli_b_qname, r_b),
    ];

    store.ingest_nodes(&ks, nodes).expect("ingest nodes");
    store.ingest_edges(&ks, edges).expect("ingest edges");

    let rows = run_query(&store);
    assert!(
        rows.is_empty(),
        "resolvers under distinct entry points must not join as forks; got {rows:?}"
    );
}

/// Test-tagged items (`is_test = true`) must be excluded — the rule's
/// `AND is_test = false` filter is load-bearing for noise suppression.
#[test]
fn test_tagged_resolvers_are_excluded() {
    let mut store = PetgraphStore::new();
    let ks = keyspace();

    let cli_qname = "vsb_fixture::Cli";
    let handle_qname = "vsb_fixture::Cli::handle";
    let prod_resolver = "vsb_fixture::Stop::stop_from_bps";
    let test_resolver = "vsb_fixture::Stop::stop_from_pct";

    let prod_node = fn_item_node(prod_resolver, "stop_from_bps");
    // Build the test-tagged node by mutating is_test.
    let mut test_node = fn_item_node(test_resolver, "stop_from_pct");
    test_node
        .props
        .insert("is_test".into(), PropValue::Bool(true));

    let nodes = vec![
        entry_point_node("Cli", cli_qname, "cli_command"),
        struct_item_node(cli_qname, "Cli"),
        fn_item_node(handle_qname, "handle"),
        prod_node,
        test_node,
    ];
    let edges = vec![
        exposes_edge(&format!("entrypoint:cli_command:{cli_qname}"), cli_qname),
        calls_edge(cli_qname, handle_qname),
        calls_edge(handle_qname, prod_resolver),
        calls_edge(handle_qname, test_resolver),
    ];

    store.ingest_nodes(&ks, nodes).expect("ingest nodes");
    store.ingest_edges(&ks, edges).expect("ingest edges");

    let rows = run_query(&store);
    assert!(
        rows.is_empty(),
        "test-tagged resolver must be excluded so the pair cannot form; got {rows:?}"
    );
}

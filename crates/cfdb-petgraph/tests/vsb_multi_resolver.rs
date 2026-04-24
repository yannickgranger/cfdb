//! VSB multi-resolver scar tests (issue #202 / RFC-036 §3.2).
//!
//! Asserts that `.cfdb/queries/vsb-multi-resolver.cypher` fires on the
//! three scar shapes enumerated in the RFC-036 test row:
//!
//! 1. **Param-Effect Canary** — MCP boundary handler registers a
//!    domain enum param and two reachable resolvers both produce the
//!    same `:Item` type. The Ctrl-Z scar from RFC-036 §3.2.
//! 2. **MCP Boundary Fix AC Template** — matches the `MCP Boundary Fix
//!    AC Template` required triple (parser delegation + schema enum
//!    derivation + error `valid_values` derivation): when the handler
//!    has *two* parser paths that both produce the same domain type,
//!    the detector fires on the split.
//! 3. **Compound Stop Layer Isolation** — compound-stop entry point
//!    registers the layered param and traverses a call chain that
//!    yields two distinct fn items both returning the same compound
//!    type. Canary-test M1 shape from the CLAUDE.md rules.
//!
//! ## Test approach — direct fact injection
//!
//! Each scar test builds a synthetic `(Vec<Node>, Vec<Edge>)` batch
//! mirroring what the HIR extractor would emit for the corresponding
//! Rust fixture under `crates/cfdb-extractor/tests/fixtures/vsb/`,
//! ingests it into a fresh `PetgraphStore`, parses the query file, and
//! runs it. Same pattern as `pattern_b_vertical_split_brain.rs` — we
//! assert the rule SHAPE fires on the expected graph shape, not that
//! extraction end-to-end is correct. Extractor correctness is covered
//! by its own test suite.
//!
//! Direct injection is faster (sub-second) and isolates the failure
//! signal: a #202 regression here cannot be blamed on HIR instability.

use std::collections::BTreeMap;
use std::path::PathBuf;

use cfdb_core::fact::{Edge, Node, PropValue};
use cfdb_core::qname::item_node_id;
use cfdb_core::schema::{EdgeLabel, Keyspace, Label};
use cfdb_core::store::StoreBackend;
use cfdb_petgraph::PetgraphStore;
use cfdb_query::parse;

// ---------------------------------------------------------------------
// Helpers — builders for the fact shapes the HIR extractor would emit.
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

fn enum_item_node(qname: &str, name: &str) -> Node {
    let mut props = BTreeMap::new();
    props.insert("qname".into(), PropValue::Str(qname.into()));
    props.insert("name".into(), PropValue::Str(name.into()));
    props.insert("kind".into(), PropValue::Str("enum".into()));
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

fn param_node(
    parent_qname: &str,
    index: i64,
    name: &str,
    type_normalized: &str,
    type_path: &str,
) -> Node {
    let mut props = BTreeMap::new();
    props.insert("index".into(), PropValue::Int(index));
    props.insert("is_self".into(), PropValue::Bool(false));
    props.insert("name".into(), PropValue::Str(name.into()));
    props.insert("parent_qname".into(), PropValue::Str(parent_qname.into()));
    props.insert(
        "type_normalized".into(),
        PropValue::Str(type_normalized.into()),
    );
    props.insert("type_path".into(), PropValue::Str(type_path.into()));
    Node {
        id: format!("param:{parent_qname}.{name}"),
        label: Label::new(Label::PARAM),
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

fn registers_param_edge(ep_id: &str, parent_qname: &str, param_name: &str) -> Edge {
    Edge {
        src: ep_id.into(),
        dst: format!("param:{parent_qname}.{param_name}"),
        label: EdgeLabel::new(EdgeLabel::REGISTERS_PARAM),
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

fn returns_edge(fn_qname: &str, return_type_qname: &str) -> Edge {
    Edge {
        src: item_node_id(fn_qname),
        dst: item_node_id(return_type_qname),
        label: EdgeLabel::new(EdgeLabel::RETURNS),
        props: BTreeMap::new(),
    }
}

fn keyspace() -> Keyspace {
    Keyspace::new("vsb-multi-resolver-test")
}

fn workspace_root() -> PathBuf {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    PathBuf::from(manifest_dir)
        .parent()
        .and_then(|p| p.parent())
        .expect("cfdb workspace root — two parents up from cfdb-petgraph/")
        .to_path_buf()
}

fn load_query_text() -> String {
    let path = workspace_root().join(".cfdb/queries/vsb-multi-resolver.cypher");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read vsb-multi-resolver.cypher at {}: {e}", path.display()))
}

fn run_query(store: &PetgraphStore) -> Vec<BTreeMap<String, cfdb_core::result::RowValue>> {
    let text = load_query_text();
    let query = parse(&text).unwrap_or_else(|e| panic!("parse vsb-multi-resolver.cypher: {e:?}"));
    let result = store
        .execute(&keyspace(), &query)
        .expect("execute vsb-multi-resolver on fresh store");
    result.rows
}

fn row_str<'a>(
    row: &'a BTreeMap<String, cfdb_core::result::RowValue>,
    key: &str,
) -> Option<&'a str> {
    row.get(key).and_then(|v| v.as_str())
}

fn row_list_len(row: &BTreeMap<String, cfdb_core::result::RowValue>, key: &str) -> Option<usize> {
    row.get(key).and_then(|v| match v {
        cfdb_core::result::RowValue::List(items) => Some(items.len()),
        _ => None,
    })
}

// ---------------------------------------------------------------------
// Scar 1 — Param-Effect Canary.
//
// An MCP handler registers a `:Param { type_normalized: "vsb_fixture::Timeframe" }`.
// Two reachable fn items return `Timeframe`:
//   - Timeframe::from_str      (string parser)
//   - Timeframe::from_capital  (provider-format parser)
// Both reachable from the single handler → one row expected.
// ---------------------------------------------------------------------

#[test]
fn param_effect_canary_emits_one_multi_resolver_row() {
    let mut store = PetgraphStore::new();
    let ks = keyspace();

    let ep_kind = "mcp_tool";
    let tool_handler = "vsb_fixture::mcp::handle_timeframe_query";
    let param_parent = tool_handler;
    let tf_type = "vsb_fixture::Timeframe";
    let r1 = "vsb_fixture::Timeframe::from_str";
    let r2 = "vsb_fixture::Timeframe::from_capital";

    let ep_id = format!("entrypoint:{ep_kind}:{tool_handler}");

    let nodes = vec![
        entry_point_node("timeframe_query", tool_handler, ep_kind),
        fn_item_node(tool_handler, "handle_timeframe_query"),
        param_node(param_parent, 0, "timeframe", tf_type, "Timeframe"),
        enum_item_node(tf_type, "Timeframe"),
        fn_item_node(r1, "from_str"),
        fn_item_node(r2, "from_capital"),
    ];
    let edges = vec![
        exposes_edge(&ep_id, tool_handler),
        registers_param_edge(&ep_id, param_parent, "timeframe"),
        calls_edge(tool_handler, r1),
        calls_edge(tool_handler, r2),
        returns_edge(r1, tf_type),
        returns_edge(r2, tf_type),
    ];

    store.ingest_nodes(&ks, nodes).expect("ingest nodes");
    store.ingest_edges(&ks, edges).expect("ingest edges");

    let rows = run_query(&store);
    assert_eq!(
        rows.len(),
        1,
        "param-effect-canary should report exactly one multi-resolver EP; rows={rows:?}"
    );
    assert_eq!(row_str(&rows[0], "entry_point"), Some("timeframe_query"));
    assert_eq!(row_str(&rows[0], "param_name"), Some("timeframe"));
    assert_eq!(row_str(&rows[0], "param_type"), Some(tf_type));
    assert_eq!(row_list_len(&rows[0], "resolvers"), Some(2));
}

// ---------------------------------------------------------------------
// Scar 2 — MCP Boundary Fix AC Template.
//
// An MCP handler registers a `Stop` param. Two resolvers: one delegating
// to the domain `FromStr`, one bypassing with a handwritten parser.
// The split-brain this models: schema enum derived from domain, but
// handler parse path diverges — both produce `Stop` and are reachable.
// ---------------------------------------------------------------------

#[test]
fn mcp_boundary_fix_ac_template_emits_one_row() {
    let mut store = PetgraphStore::new();
    let ks = keyspace();

    let ep_kind = "mcp_tool";
    let handler = "vsb_fixture::mcp::handle_stop_request";
    let stop_type = "vsb_fixture::Stop";
    let r_domain = "vsb_fixture::Stop::from_str";
    let r_manual = "vsb_fixture::mcp::parse_stop_raw";

    let ep_id = format!("entrypoint:{ep_kind}:{handler}");

    let nodes = vec![
        entry_point_node("stop_request", handler, ep_kind),
        fn_item_node(handler, "handle_stop_request"),
        param_node(handler, 0, "stop", stop_type, "Stop"),
        enum_item_node(stop_type, "Stop"),
        fn_item_node(r_domain, "from_str"),
        fn_item_node(r_manual, "parse_stop_raw"),
    ];
    let edges = vec![
        exposes_edge(&ep_id, handler),
        registers_param_edge(&ep_id, handler, "stop"),
        calls_edge(handler, r_domain),
        calls_edge(handler, r_manual),
        returns_edge(r_domain, stop_type),
        returns_edge(r_manual, stop_type),
    ];

    store.ingest_nodes(&ks, nodes).expect("ingest nodes");
    store.ingest_edges(&ks, edges).expect("ingest edges");

    let rows = run_query(&store);
    assert_eq!(
        rows.len(),
        1,
        "mcp-boundary-fix-ac should report exactly one multi-resolver EP; rows={rows:?}"
    );
    assert_eq!(row_str(&rows[0], "param_type"), Some(stop_type));
    assert_eq!(row_list_len(&rows[0], "resolvers"), Some(2));
}

// ---------------------------------------------------------------------
// Scar 3 — Compound Stop Layer Isolation.
//
// Entry point (CLI command) registers `CompoundStop` param. Traverses
// a 2-hop CALLS chain; two fn items return `CompoundStop`:
//   - CompoundStop::new         (modern builder)
//   - CompoundStop::from_legacy (legacy path kept for back-compat)
// Expected: one row.
// ---------------------------------------------------------------------

#[test]
fn compound_stop_layer_isolation_emits_one_row() {
    let mut store = PetgraphStore::new();
    let ks = keyspace();

    let ep_kind = "cli_command";
    let cli_handler = "vsb_fixture::cli::RunStop::handle";
    let mid_fn = "vsb_fixture::cli::build_stop_config";
    let cs_type = "vsb_fixture::CompoundStop";
    let r_new = "vsb_fixture::CompoundStop::new";
    let r_legacy = "vsb_fixture::CompoundStop::from_legacy";

    let ep_id = format!("entrypoint:{ep_kind}:{cli_handler}");

    let nodes = vec![
        entry_point_node("run-stop", cli_handler, ep_kind),
        fn_item_node(cli_handler, "handle"),
        fn_item_node(mid_fn, "build_stop_config"),
        param_node(cli_handler, 0, "compound_stop", cs_type, "CompoundStop"),
        struct_item_node(cs_type, "CompoundStop"),
        fn_item_node(r_new, "new"),
        fn_item_node(r_legacy, "from_legacy"),
    ];
    let edges = vec![
        exposes_edge(&ep_id, cli_handler),
        registers_param_edge(&ep_id, cli_handler, "compound_stop"),
        // Two-hop chain — mid_fn between handler and the resolvers.
        calls_edge(cli_handler, mid_fn),
        calls_edge(mid_fn, r_new),
        calls_edge(mid_fn, r_legacy),
        returns_edge(r_new, cs_type),
        returns_edge(r_legacy, cs_type),
    ];

    store.ingest_nodes(&ks, nodes).expect("ingest nodes");
    store.ingest_edges(&ks, edges).expect("ingest edges");

    let rows = run_query(&store);
    assert_eq!(
        rows.len(),
        1,
        "compound-stop-layer-isolation should report one multi-resolver EP; rows={rows:?}"
    );
    assert_eq!(row_list_len(&rows[0], "resolvers"), Some(2));
}

// ---------------------------------------------------------------------
// Negative 1 — single resolver must NOT fire.
//
// One clean entry point with one reachable resolver. Query returns zero
// rows — `size(resolvers) > 1` is load-bearing.
// ---------------------------------------------------------------------

#[test]
fn single_resolver_per_entry_point_emits_no_rows() {
    let mut store = PetgraphStore::new();
    let ks = keyspace();

    let ep_kind = "mcp_tool";
    let handler = "vsb_fixture::mcp::handle_clean";
    let t_type = "vsb_fixture::Clean";
    let resolver = "vsb_fixture::Clean::from_str";

    let ep_id = format!("entrypoint:{ep_kind}:{handler}");

    let nodes = vec![
        entry_point_node("clean_tool", handler, ep_kind),
        fn_item_node(handler, "handle_clean"),
        param_node(handler, 0, "clean", t_type, "Clean"),
        enum_item_node(t_type, "Clean"),
        fn_item_node(resolver, "from_str"),
    ];
    let edges = vec![
        exposes_edge(&ep_id, handler),
        registers_param_edge(&ep_id, handler, "clean"),
        calls_edge(handler, resolver),
        returns_edge(resolver, t_type),
    ];

    store.ingest_nodes(&ks, nodes).expect("ingest nodes");
    store.ingest_edges(&ks, edges).expect("ingest edges");

    let rows = run_query(&store);
    assert!(
        rows.is_empty(),
        "single resolver must not be reported; rows={rows:?}"
    );
}

// ---------------------------------------------------------------------
// Negative 2 — two resolvers under DIFFERENT entry points are not a
// fork. The per-EP join excludes this shape by construction.
// ---------------------------------------------------------------------

#[test]
fn two_resolvers_split_across_entry_points_emit_no_rows() {
    let mut store = PetgraphStore::new();
    let ks = keyspace();

    let ep_kind = "mcp_tool";
    let handler_a = "vsb_fixture::mcp::handle_a";
    let handler_b = "vsb_fixture::mcp::handle_b";
    let t_type = "vsb_fixture::Timeframe";
    let r_a = "vsb_fixture::Timeframe::from_str";
    let r_b = "vsb_fixture::Timeframe::from_capital";

    let ep_a_id = format!("entrypoint:{ep_kind}:{handler_a}");
    let ep_b_id = format!("entrypoint:{ep_kind}:{handler_b}");

    let nodes = vec![
        entry_point_node("tool_a", handler_a, ep_kind),
        entry_point_node("tool_b", handler_b, ep_kind),
        fn_item_node(handler_a, "handle_a"),
        fn_item_node(handler_b, "handle_b"),
        param_node(handler_a, 0, "tf", t_type, "Timeframe"),
        param_node(handler_b, 0, "tf", t_type, "Timeframe"),
        enum_item_node(t_type, "Timeframe"),
        fn_item_node(r_a, "from_str"),
        fn_item_node(r_b, "from_capital"),
    ];
    let edges = vec![
        exposes_edge(&ep_a_id, handler_a),
        exposes_edge(&ep_b_id, handler_b),
        registers_param_edge(&ep_a_id, handler_a, "tf"),
        registers_param_edge(&ep_b_id, handler_b, "tf"),
        calls_edge(handler_a, r_a),
        calls_edge(handler_b, r_b),
        returns_edge(r_a, t_type),
        returns_edge(r_b, t_type),
    ];

    store.ingest_nodes(&ks, nodes).expect("ingest nodes");
    store.ingest_edges(&ks, edges).expect("ingest edges");

    let rows = run_query(&store);
    assert!(
        rows.is_empty(),
        "resolvers under distinct entry points must not join; rows={rows:?}"
    );
}

// ---------------------------------------------------------------------
// Negative 3 — resolver that returns a DIFFERENT type from the
// registered param's `type_normalized` must not form a fork. The
// `t.qname = p.type_normalized` filter is load-bearing.
// ---------------------------------------------------------------------

#[test]
fn resolvers_returning_mismatched_type_emit_no_rows() {
    let mut store = PetgraphStore::new();
    let ks = keyspace();

    let ep_kind = "mcp_tool";
    let handler = "vsb_fixture::mcp::handle_mixed";
    let registered_type = "vsb_fixture::Timeframe";
    let other_type = "vsb_fixture::Duration";
    let r_match = "vsb_fixture::Timeframe::from_str";
    let r_other = "vsb_fixture::Duration::from_millis";

    let ep_id = format!("entrypoint:{ep_kind}:{handler}");

    let nodes = vec![
        entry_point_node("mixed_tool", handler, ep_kind),
        fn_item_node(handler, "handle_mixed"),
        param_node(handler, 0, "tf", registered_type, "Timeframe"),
        enum_item_node(registered_type, "Timeframe"),
        struct_item_node(other_type, "Duration"),
        fn_item_node(r_match, "from_str"),
        fn_item_node(r_other, "from_millis"),
    ];
    let edges = vec![
        exposes_edge(&ep_id, handler),
        registers_param_edge(&ep_id, handler, "tf"),
        calls_edge(handler, r_match),
        calls_edge(handler, r_other),
        returns_edge(r_match, registered_type),
        returns_edge(r_other, other_type),
    ];

    store.ingest_nodes(&ks, nodes).expect("ingest nodes");
    store.ingest_edges(&ks, edges).expect("ingest edges");

    let rows = run_query(&store);
    assert!(
        rows.is_empty(),
        "only one resolver returns the registered type — must not fire; rows={rows:?}"
    );
}

//! Pattern B `drop` kind scar tests (issue #297 Phase B / #44 follow-up).
//!
//! Asserts that `examples/queries/vertical-split-brain-drop.cypher`
//! fires on the qbot-core #2651 compound-stop param-drop shape: one
//! `:EntryPoint` registers a wire-level param key K, two resolvers
//! reachable from the handler — one reads K, the other reads a
//! divergent key K' that the entry point never registers.
//!
//! ## Test approach — direct fact injection (mirrors the `fork` tests)
//!
//! Same rationale as `pattern_b_vertical_split_brain.rs`: build
//! synthetic `(Vec<Node>, Vec<Edge>)` batches matching what the HIR
//! extractor would emit, ingest into a fresh `PetgraphStore`, parse +
//! run the query. The on-disk fixture under
//! `examples/queries/fixtures/vertical-split-brain-drop/` exists for
//! human verification (per the sister fixture's convention) — these
//! tests are the regression surface.
//!
//! ## What the rule keys on
//!
//! - `:EntryPoint -[:REGISTERS_PARAM]-> wire` (target may be `:Field`,
//!   `:Variant`, or `:Param` per RFC-037 §3.1 — the rule does not
//!   restrict the target label)
//! - `(handler:Item)` exposed by the entry point
//! - Two reachable resolvers (`layer_k`, `layer_kp1`) via
//!   `[:CALLS*1..8]` whose `:Param.name` differs from each other
//! - `matched.name = wire.name` for layer K
//! - `divergent.name <> wire.name` for layer K+1
//! - `NOT EXISTS { ep -[:REGISTERS_PARAM]-> other_wire WHERE
//!    other_wire.name = divergent.name }` — the divergent key is
//!   genuinely unwired

use std::collections::BTreeMap;
use std::path::PathBuf;

use cfdb_core::fact::{Edge, Node, PropValue};
use cfdb_core::qname::{field_node_id, item_node_id, param_node_id};
use cfdb_core::schema::{EdgeLabel, Keyspace, Label};
use cfdb_core::store::StoreBackend;
use cfdb_petgraph::PetgraphStore;
use cfdb_query::parse;

// ---------------------------------------------------------------------
// Helpers — small, test-local builders for the fact shapes the HIR
// extractor would emit. Mirror of pattern_b_vertical_split_brain.rs's
// helpers + param/field/has_param/registers_param.
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
    props.insert("crate".into(), PropValue::Str("vsb_drop_fixture".into()));
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
    props.insert("crate".into(), PropValue::Str("vsb_drop_fixture".into()));
    props.insert("file".into(), PropValue::Str("synthetic".into()));
    props.insert("line".into(), PropValue::Int(0));
    props.insert("is_test".into(), PropValue::Bool(false));
    Node {
        id: item_node_id(qname),
        label: Label::new(Label::ITEM),
        props,
    }
}

/// `:Field` node — clap struct field; what HIR emits as the
/// `:EntryPoint -[:REGISTERS_PARAM]-> :Field` target.
fn field_node(parent_qname: &str, field_name: &str) -> Node {
    let mut props = BTreeMap::new();
    props.insert("name".into(), PropValue::Str(field_name.into()));
    props.insert("parent_qname".into(), PropValue::Str(parent_qname.into()));
    Node {
        id: field_node_id(parent_qname, field_name),
        label: Label::new(Label::FIELD),
        props,
    }
}

/// `:Param` node — fn/method parameter; what HIR + syn emit as the
/// `:Item -[:HAS_PARAM]-> :Param` target. The node id is keyed by
/// `(parent_qname, index)` per `cfdb_core::qname::param_node_id` —
/// positionally stable within a single extract.
fn param_node(parent_qname: &str, param_name: &str, index: usize) -> Node {
    let mut props = BTreeMap::new();
    props.insert("name".into(), PropValue::Str(param_name.into()));
    props.insert("parent_qname".into(), PropValue::Str(parent_qname.into()));
    props.insert(
        "index".into(),
        PropValue::Int(i64::try_from(index).expect("param index fits i64")),
    );
    Node {
        id: param_node_id(parent_qname, index),
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

fn has_param_edge(item_qname: &str, param_parent_qname: &str, param_index: usize) -> Edge {
    Edge {
        src: item_node_id(item_qname),
        dst: param_node_id(param_parent_qname, param_index),
        label: EdgeLabel::new(EdgeLabel::HAS_PARAM),
        props: BTreeMap::new(),
    }
}

fn registers_param_field_edge(ep_id: &str, parent_qname: &str, field_name: &str) -> Edge {
    Edge {
        src: ep_id.into(),
        dst: field_node_id(parent_qname, field_name),
        label: EdgeLabel::new(EdgeLabel::REGISTERS_PARAM),
        props: BTreeMap::new(),
    }
}

fn keyspace() -> Keyspace {
    Keyspace::new("vsb-drop-test")
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
    let path = workspace_root().join("examples/queries/vertical-split-brain-drop.cypher");
    std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "read vertical-split-brain-drop.cypher at {}: {e}",
            path.display()
        )
    })
}

fn run_query(store: &PetgraphStore) -> Vec<BTreeMap<String, cfdb_core::result::RowValue>> {
    let text = load_query_text();
    let query =
        parse(&text).unwrap_or_else(|e| panic!("parse vertical-split-brain-drop.cypher: {e:?}"));
    let result = store
        .execute(&keyspace(), &query)
        .expect("execute vertical-split-brain-drop on fresh store");
    result.rows
}

fn row_str<'a>(
    row: &'a BTreeMap<String, cfdb_core::result::RowValue>,
    key: &str,
) -> Option<&'a str> {
    row.get(key).and_then(|v| v.as_str())
}

// ---------------------------------------------------------------------
// Fixture builder — the qbot-core #2651 compound-stop drop shape.
// ---------------------------------------------------------------------

/// Build the canonical drop-shape graph: `Cli` registers the wire
/// param `wire_field`; both `layer_k_qname` and `layer_kp1_qname` are
/// reachable from `Cli::handle` via `Engine::dispatch`; layer K reads
/// a `:Param` matching `wire_field`, layer K+1 reads `divergent_param`
/// (which is NOT wire-registered).
struct DropShape {
    cli_qname: &'static str,
    handle_qname: &'static str,
    dispatch_qname: &'static str,
    layer_k_qname: &'static str,
    layer_kp1_qname: &'static str,
    wire_field: &'static str,
    divergent_param: &'static str,
}

const CANONICAL_SHAPE: DropShape = DropShape {
    cli_qname: "vsb_drop_fixture::Cli",
    handle_qname: "vsb_drop_fixture::Cli::handle",
    dispatch_qname: "vsb_drop_fixture::Engine::dispatch",
    layer_k_qname: "vsb_drop_fixture::compute_active_mult",
    layer_kp1_qname: "vsb_drop_fixture::compute_trail_layer",
    wire_field: "stop_atr_mult",
    divergent_param: "active_mult",
};

fn build_drop_shape(shape: &DropShape) -> (Vec<Node>, Vec<Edge>) {
    let ep_id = format!("entrypoint:cli_command:{}", shape.cli_qname);

    let nodes = vec![
        entry_point_node("Cli", shape.cli_qname, "cli_command"),
        struct_item_node(shape.cli_qname, "Cli"),
        field_node(shape.cli_qname, shape.wire_field),
        fn_item_node(shape.handle_qname, "handle"),
        fn_item_node(shape.dispatch_qname, "dispatch"),
        fn_item_node(shape.layer_k_qname, "compute_active_mult"),
        fn_item_node(shape.layer_kp1_qname, "compute_trail_layer"),
        // Layer K reads the wire key (matched).
        param_node(shape.layer_k_qname, shape.wire_field, 0),
        // Layer K+1 reads the divergent key.
        param_node(shape.layer_kp1_qname, shape.divergent_param, 0),
    ];

    let edges = vec![
        registers_param_field_edge(&ep_id, shape.cli_qname, shape.wire_field),
        exposes_edge(&ep_id, shape.cli_qname),
        // CALLS chain: Cli -> handle -> dispatch -> { layer_k, layer_kp1 }.
        calls_edge(shape.cli_qname, shape.handle_qname),
        calls_edge(shape.handle_qname, shape.dispatch_qname),
        calls_edge(shape.dispatch_qname, shape.layer_k_qname),
        calls_edge(shape.dispatch_qname, shape.layer_kp1_qname),
        // HAS_PARAM edges to the resolver fns' params (param_node_id
        // keys by (parent_qname, index); both fns have one param at
        // index 0).
        has_param_edge(shape.layer_k_qname, shape.layer_k_qname, 0),
        has_param_edge(shape.layer_kp1_qname, shape.layer_kp1_qname, 0),
    ];

    (nodes, edges)
}

fn ingest(store: &mut PetgraphStore, nodes: Vec<Node>, edges: Vec<Edge>) {
    let ks = keyspace();
    store
        .ingest_nodes(&ks, nodes)
        .expect("ingest synthetic nodes");
    store
        .ingest_edges(&ks, edges)
        .expect("ingest synthetic edges");
}

// ---------------------------------------------------------------------
// Scar tests
// ---------------------------------------------------------------------

/// Positive scar — the canonical qbot-core #2651 compound-stop drop
/// shape MUST fire the rule, returning exactly one row with the
/// expected output columns.
#[test]
fn scar_2651_compound_stop_drop_emits_one_drop_row() {
    let mut store = PetgraphStore::new();
    let (nodes, edges) = build_drop_shape(&CANONICAL_SHAPE);
    ingest(&mut store, nodes, edges);

    let rows = run_query(&store);
    assert_eq!(
        rows.len(),
        1,
        "expected exactly 1 drop row for #2651 canonical shape; got {}: {:?}",
        rows.len(),
        rows
    );

    let row = &rows[0];
    assert_eq!(row_str(row, "entry_point"), Some("Cli"));
    assert_eq!(row_str(row, "entry_qname"), Some(CANONICAL_SHAPE.cli_qname));
    assert_eq!(row_str(row, "wire_param"), Some(CANONICAL_SHAPE.wire_field));
    assert_eq!(
        row_str(row, "matching_resolver"),
        Some(CANONICAL_SHAPE.layer_k_qname)
    );
    assert_eq!(
        row_str(row, "divergent_resolver"),
        Some(CANONICAL_SHAPE.layer_kp1_qname)
    );
    assert_eq!(
        row_str(row, "divergent_key"),
        Some(CANONICAL_SHAPE.divergent_param)
    );
    assert_eq!(row_str(row, "divergence_kind"), Some("drop"));
}

/// Negative — both resolvers read the SAME key (no divergence). Rule
/// must NOT fire.
#[test]
fn both_resolvers_read_wire_key_emits_no_rows() {
    let mut store = PetgraphStore::new();
    let shape = DropShape {
        // Both resolvers read `stop_atr_mult` — no drop.
        divergent_param: "stop_atr_mult",
        ..CANONICAL_SHAPE
    };
    let (nodes, edges) = build_drop_shape(&shape);
    ingest(&mut store, nodes, edges);

    let rows = run_query(&store);
    assert_eq!(
        rows.len(),
        0,
        "two resolvers reading the same wire key is not a drop; got: {rows:?}"
    );
}

/// Documented false-positive class — when the "divergent" key is
/// ALSO wire-registered, the rule still fires because the cfdb-query
/// v0.1 subset's `NOT EXISTS` cannot bind outer-scope variables.
/// Two rows fire (one per direction of the symmetric K/K' pair).
/// The cypher header documents the operator's manual triage
/// procedure for this class. Pinning the current behaviour here so
/// that a v0.2 query-subset upgrade (outer-scope binding inside
/// `NOT EXISTS`) flips this test red and forces the rule to be
/// tightened — that's the desired pattern, not a regression.
#[test]
fn both_keys_wire_registered_currently_fires_as_known_false_positive() {
    let mut store = PetgraphStore::new();
    let (mut nodes, mut edges) = build_drop_shape(&CANONICAL_SHAPE);
    let ep_id = format!("entrypoint:cli_command:{}", CANONICAL_SHAPE.cli_qname);

    // Add a SECOND :Field on the Cli struct named `active_mult` — i.e.
    // the user supplies BOTH at the wire form (legitimate "compound
    // stop accepts both keys" shape).
    nodes.push(field_node(CANONICAL_SHAPE.cli_qname, "active_mult"));
    edges.push(registers_param_field_edge(
        &ep_id,
        CANONICAL_SHAPE.cli_qname,
        "active_mult",
    ));

    ingest(&mut store, nodes, edges);

    let rows = run_query(&store);
    assert_eq!(
        rows.len(),
        2,
        "v0.1 cypher subset: NOT EXISTS can't bind outer scope; the \
         rule fires on both K/K' directions when both are wire-\
         registered. Operator triages manually per the cypher header. \
         When the subset gains outer-scope NOT EXISTS this test \
         flips — tighten the rule + reduce expected count to 0. \
         Got: {rows:?}"
    );
}

/// Negative — only ONE resolver reachable. The rule's two-resolver
/// join cannot bind, so no rows.
#[test]
fn single_reachable_resolver_emits_no_rows() {
    let mut store = PetgraphStore::new();
    let (mut nodes, mut edges) = build_drop_shape(&CANONICAL_SHAPE);

    // Drop the layer-K+1 fn + its CALLS edge + its HAS_PARAM + the
    // divergent param node so only `compute_active_mult` is reachable.
    nodes.retain(|n| {
        n.id != item_node_id(CANONICAL_SHAPE.layer_kp1_qname)
            && n.id != param_node_id(CANONICAL_SHAPE.layer_kp1_qname, 0)
    });
    edges.retain(|e| {
        e.src != item_node_id(CANONICAL_SHAPE.dispatch_qname)
            || e.dst != item_node_id(CANONICAL_SHAPE.layer_kp1_qname)
    });
    edges.retain(|e| e.src != item_node_id(CANONICAL_SHAPE.layer_kp1_qname));

    ingest(&mut store, nodes, edges);

    let rows = run_query(&store);
    assert_eq!(
        rows.len(),
        0,
        "only one reachable resolver → no two-way join → no rows; got: {rows:?}"
    );
}

/// Negative — the layer-K+1 resolver IS test code (`is_test = true`).
/// Rule's `is_test = false` filter excludes it; no rows.
#[test]
fn test_layer_kp1_resolver_is_excluded() {
    let mut store = PetgraphStore::new();
    let (mut nodes, edges) = build_drop_shape(&CANONICAL_SHAPE);

    // Mark layer_kp1 as a test fn — it should be filtered out.
    for node in &mut nodes {
        if node.id == item_node_id(CANONICAL_SHAPE.layer_kp1_qname) {
            node.props.insert("is_test".into(), PropValue::Bool(true));
        }
    }

    ingest(&mut store, nodes, edges);

    let rows = run_query(&store);
    assert_eq!(
        rows.len(),
        0,
        "test layer should be filtered; got: {rows:?}"
    );
}

use std::collections::BTreeMap;
use std::path::PathBuf;

use cfdb_core::fact::{Edge, Node, PropValue};
use cfdb_core::qname::item_node_id;
use cfdb_core::query::ParamBinding;
use cfdb_core::schema::{EdgeLabel, Keyspace, Label};
use cfdb_core::store::{QueryBackend, StoreBackend};
use cfdb_eval::QueryEngine;
use cfdb_petgraph::PetgraphStore;
use cfdb_query::parse;

fn item_node(qname: &str, name: &str, kind: &str, crate_name: &str) -> Node {
    let mut props = BTreeMap::new();
    props.insert("qname".into(), PropValue::Str(qname.into()));
    props.insert("name".into(), PropValue::Str(name.into()));
    props.insert("kind".into(), PropValue::Str(kind.into()));
    props.insert("crate".into(), PropValue::Str(crate_name.into()));
    props.insert("file".into(), PropValue::Str("synthetic".into()));
    props.insert("line".into(), PropValue::Int(0));
    props.insert("is_test".into(), PropValue::Bool(false));
    Node {
        id: item_node_id(qname),
        label: Label::new(Label::ITEM),
        props,
    }
}

fn fn_item_with_metrics(
    qname: &str,
    name: &str,
    crate_name: &str,
    unwrap_count: i64,
    test_coverage: f64,
) -> Node {
    let mut n = item_node(qname, name, "fn", crate_name);
    n.props
        .insert("unwrap_count".into(), PropValue::Int(unwrap_count));
    n.props
        .insert("test_coverage".into(), PropValue::Float(test_coverage));
    n
}

fn concept_node(concept_name: &str) -> Node {
    let mut props = BTreeMap::new();
    props.insert("name".into(), PropValue::Str(concept_name.into()));
    Node {
        id: format!("concept:{concept_name}"),
        label: Label::new(Label::CONCEPT),
        props,
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

fn labeled_as_edge(item_qname: &str, concept_name: &str) -> Edge {
    Edge {
        src: item_node_id(item_qname),
        dst: format!("concept:{concept_name}"),
        label: EdgeLabel::new(EdgeLabel::LABELED_AS),
        props: BTreeMap::new(),
    }
}

fn canonical_for_edge(item_qname: &str, concept_name: &str) -> Edge {
    Edge {
        src: item_node_id(item_qname),
        dst: format!("concept:{concept_name}"),
        label: EdgeLabel::new(EdgeLabel::CANONICAL_FOR),
        props: BTreeMap::new(),
    }
}

fn keyspace() -> Keyspace {
    Keyspace::new("raid-plan-test")
}

fn workspace_root() -> PathBuf {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    PathBuf::from(manifest_dir)
        .parent()
        .and_then(|p| p.parent())
        .expect("cfdb workspace root — two parents up from cfdb-petgraph/")
        .to_path_buf()
}

fn load_query(relative_path: &str) -> cfdb_core::Query {
    let path = workspace_root()
        .join("examples/queries/raid")
        .join(relative_path);
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    parse(&text).unwrap_or_else(|e| panic!("parse {}: {e:?}", path.display()))
}

fn list_param(items: &[&str]) -> ParamBinding {
    ParamBinding::List(items.iter().map(|s| PropValue::Str((*s).into())).collect())
}

fn build_fixture() -> PetgraphStore {
    let mut store = PetgraphStore::new();
    let ks = keyspace();

    let nodes = vec![
        item_node(
            "stop_engine::types::StopLoss",
            "StopLoss",
            "struct",
            "stop_engine",
        ),
        item_node(
            "stop_engine::types::TrailingStop",
            "TrailingStop",
            "struct",
            "stop_engine",
        ),
        item_node(
            "stop_engine::mcp::handle_request",
            "handle_request",
            "fn",
            "stop_engine",
        ),
        item_node(
            "stop_engine::legacy::LegacyBuilder",
            "LegacyBuilder",
            "struct",
            "stop_engine",
        ),
        item_node(
            "stop_engine::legacy::parse_bps",
            "parse_bps",
            "fn",
            "stop_engine",
        ),
        fn_item_with_metrics(
            "stop_engine::risky::unwrap_stop",
            "unwrap_stop",
            "stop_engine",
            5,
            0.80,
        ),
        item_node(
            "stop_engine::util::format_price",
            "format_price",
            "fn",
            "stop_engine",
        ),
        item_node(
            "stop_engine::ratio::compute",
            "compute",
            "fn",
            "stop_engine",
        ),
        item_node(
            "consumer_app::main::use_trailing",
            "use_trailing",
            "fn",
            "consumer_app",
        ),
        concept_node("compound_stop"),
        concept_node("risk_ratio"),
    ];

    let edges = vec![
        calls_edge(
            "stop_engine::types::StopLoss",
            "stop_engine::legacy::LegacyBuilder",
        ),
        calls_edge(
            "consumer_app::main::use_trailing",
            "stop_engine::types::TrailingStop",
        ),
        labeled_as_edge("stop_engine::risky::unwrap_stop", "compound_stop"),
        canonical_for_edge("stop_engine::ratio::compute", "risk_ratio"),
    ];

    store.ingest_nodes(&ks, nodes).expect("ingest nodes");
    store.ingest_edges(&ks, edges).expect("ingest edges");
    store
}

fn bind_plan(query: &mut cfdb_core::Query) {
    query.params.insert(
        "source_context".into(),
        ParamBinding::Scalar(PropValue::Str("stop_engine".into())),
    );
    query.params.insert(
        "portage".into(),
        list_param(&[
            "stop_engine::risky::unwrap_stop",
            "stop_engine::types::StopLoss",
            "stop_engine::types::TrailingStop",
        ]),
    );
    query.params.insert(
        "rewrite".into(),
        list_param(&["compound_stop", "risk_ratio"]),
    );
    query.params.insert(
        "glue".into(),
        list_param(&["stop_engine::mcp::handle_request"]),
    );
    query.params.insert(
        "drop".into(),
        list_param(&[
            "stop_engine::legacy::LegacyBuilder",
            "stop_engine::legacy::parse_bps",
        ]),
    );
}

fn run(
    store: &PetgraphStore,
    query: &cfdb_core::Query,
) -> Vec<BTreeMap<String, cfdb_core::result::RowValue>> {
    QueryEngine::new(store)
        .execute(&keyspace(), query)
        .expect("execute raid query")
        .rows
}

fn row_str<'a>(
    row: &'a BTreeMap<String, cfdb_core::result::RowValue>,
    key: &str,
) -> Option<&'a str> {
    row.get(key).and_then(|v| v.as_str())
}

#[test]
fn completeness_flags_items_not_in_any_qname_bucket() {
    let store = build_fixture();
    let mut q = load_query("raid-completeness.cypher");
    bind_plan(&mut q);

    let rows = run(&store, &q);

    let qnames: Vec<&str> = rows.iter().filter_map(|r| row_str(r, "qname")).collect();
    assert!(
        qnames.contains(&"stop_engine::util::format_price"),
        "completeness must flag the unclaimed format_price item; got {qnames:?}"
    );
    assert!(
        qnames.contains(&"stop_engine::ratio::compute"),
        "completeness flags unplaced canonical — triaged by adding to portage; got {qnames:?}"
    );
    assert!(
        !qnames.contains(&"stop_engine::types::StopLoss"),
        "portage items must not flag as unclaimed; got {qnames:?}"
    );
    assert!(
        !qnames.contains(&"stop_engine::legacy::LegacyBuilder"),
        "drop items must not flag as unclaimed; got {qnames:?}"
    );
    assert!(
        !qnames.contains(&"stop_engine::mcp::handle_request"),
        "glue items must not flag as unclaimed; got {qnames:?}"
    );
    assert!(
        !qnames.contains(&"stop_engine::risky::unwrap_stop"),
        "portage item unwrap_stop must not flag as unclaimed; got {qnames:?}"
    );
}

#[test]
fn dangling_drop_flags_the_still_called_drop() {
    let store = build_fixture();
    let mut q = load_query("raid-dangling-drop.cypher");
    bind_plan(&mut q);

    let rows = run(&store, &q);

    assert_eq!(
        rows.len(),
        1,
        "dangling-drop should emit exactly one row; rows={rows:?}"
    );
    assert_eq!(
        row_str(&rows[0], "dropped_qname"),
        Some("stop_engine::legacy::LegacyBuilder")
    );
    assert_eq!(
        row_str(&rows[0], "caller_qname"),
        Some("stop_engine::types::StopLoss")
    );
}

#[test]
fn hidden_callers_flags_external_caller_of_portage() {
    let store = build_fixture();
    let mut q = load_query("raid-hidden-callers.cypher");
    bind_plan(&mut q);

    let rows = run(&store, &q);

    assert_eq!(
        rows.len(),
        1,
        "hidden-callers should emit one row; rows={rows:?}"
    );
    assert_eq!(
        row_str(&rows[0], "portaged_qname"),
        Some("stop_engine::types::TrailingStop")
    );
    assert_eq!(
        row_str(&rows[0], "external_qname"),
        Some("consumer_app::main::use_trailing")
    );
    assert_eq!(row_str(&rows[0], "external_crate"), Some("consumer_app"));
}

#[test]
fn missing_canonical_flags_rewrite_without_canonical_for_target() {
    let store = build_fixture();
    let mut q = load_query("raid-missing-canonical.cypher");
    bind_plan(&mut q);

    let rows = run(&store, &q);

    assert_eq!(
        rows.len(),
        1,
        "missing-canonical should emit one row; rows={rows:?}"
    );
    assert_eq!(row_str(&rows[0], "concept_name"), Some("compound_stop"));
}

#[test]
fn signal_mismatch_flags_unclean_portage_item() {
    let store = build_fixture();
    let mut q = load_query("raid-signal-mismatch.cypher");
    bind_plan(&mut q);
    q.params.insert(
        "max_unwraps".into(),
        ParamBinding::Scalar(PropValue::Int(0)),
    );
    q.params.insert(
        "min_coverage".into(),
        ParamBinding::Scalar(PropValue::Float(0.60)),
    );

    let rows = run(&store, &q);

    assert_eq!(
        rows.len(),
        1,
        "signal-mismatch should emit one row; rows={rows:?}"
    );
    assert_eq!(
        row_str(&rows[0], "qname"),
        Some("stop_engine::risky::unwrap_stop")
    );
}

fn bind_clean_plan(query: &mut cfdb_core::Query) {
    query.params.insert(
        "source_context".into(),
        ParamBinding::Scalar(PropValue::Str("stop_engine".into())),
    );
    query.params.insert(
        "portage".into(),
        list_param(&[
            "stop_engine::types::StopLoss",
            "stop_engine::types::TrailingStop",
            "stop_engine::risky::unwrap_stop",
            "stop_engine::util::format_price",
            "stop_engine::ratio::compute",
        ]),
    );
    query
        .params
        .insert("rewrite".into(), list_param(&["risk_ratio"]));
    query.params.insert(
        "glue".into(),
        list_param(&["stop_engine::mcp::handle_request"]),
    );
    query.params.insert(
        "drop".into(),
        list_param(&["stop_engine::legacy::parse_bps"]),
    );
}

#[test]
fn dangling_drop_is_empty_on_clean_plan() {
    let store = build_fixture();
    let mut q = load_query("raid-dangling-drop.cypher");
    bind_clean_plan(&mut q);
    let rows = run(&store, &q);
    assert!(
        rows.is_empty(),
        "clean plan must not flag dangling-drop; rows={rows:?}"
    );
}

#[test]
fn signal_mismatch_is_empty_when_thresholds_relaxed() {
    let store = build_fixture();
    let mut q = load_query("raid-signal-mismatch.cypher");
    bind_plan(&mut q);
    q.params.insert(
        "max_unwraps".into(),
        ParamBinding::Scalar(PropValue::Int(10)),
    );
    q.params.insert(
        "min_coverage".into(),
        ParamBinding::Scalar(PropValue::Float(0.10)),
    );
    let rows = run(&store, &q);
    assert!(
        rows.is_empty(),
        "relaxed thresholds must clear signal-mismatch; rows={rows:?}"
    );
}

use std::collections::BTreeMap;
use std::path::PathBuf;

use cfdb_core::fact::{Node, PropValue};
use cfdb_core::qname::item_node_id;
use cfdb_core::schema::{Keyspace, Label};
use cfdb_core::store::{QueryBackend, StoreBackend};
use cfdb_eval::QueryEngine;
use cfdb_petgraph::PetgraphStore;
use cfdb_query::parse;

fn fn_item(
    qname: &str,
    name: &str,
    signature_hash: &str,
    dup_cluster_id: Option<&str>,
    is_test: bool,
) -> Node {
    let mut props = BTreeMap::new();
    props.insert("qname".into(), PropValue::Str(qname.into()));
    props.insert("name".into(), PropValue::Str(name.into()));
    props.insert("kind".into(), PropValue::Str("fn".into()));
    props.insert("crate".into(), PropValue::Str("hsb_fixture".into()));
    props.insert("file".into(), PropValue::Str("synthetic".into()));
    props.insert("line".into(), PropValue::Int(0));
    props.insert("is_test".into(), PropValue::Bool(is_test));
    props.insert(
        "signature_hash".into(),
        PropValue::Str(signature_hash.into()),
    );
    if let Some(cid) = dup_cluster_id {
        props.insert("dup_cluster_id".into(), PropValue::Str(cid.into()));
    }
    Node {
        id: item_node_id(qname),
        label: Label::new(Label::ITEM),
        props,
    }
}

fn keyspace() -> Keyspace {
    Keyspace::new("hsb-cluster-test")
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
    let path = workspace_root().join(".cfdb/queries/hsb-cluster.cypher");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read hsb-cluster.cypher at {}: {e}", path.display()))
}

fn run_query(store: &PetgraphStore) -> Vec<BTreeMap<String, cfdb_core::result::RowValue>> {
    let text = load_query_text();
    let query = parse(&text).unwrap_or_else(|e| panic!("parse hsb-cluster.cypher: {e:?}"));
    let result = QueryEngine::new(store)
        .execute(&keyspace(), &query)
        .expect("execute hsb-cluster on fresh store");
    result.rows
}

fn row_str<'a>(
    row: &'a BTreeMap<String, cfdb_core::result::RowValue>,
    key: &str,
) -> Option<&'a str> {
    row.get(key).and_then(|v| v.as_str())
}

#[test]
fn same_name_duplicate_fns_emit_one_cluster_row() {
    let mut store = PetgraphStore::new();
    let ks = keyspace();

    let cid = "sha-same-name-cluster";
    let a = "crate_a::domain::make_stop";
    let b = "crate_b::domain::make_stop";

    let nodes = vec![
        fn_item(a, "make_stop", "sig-1", Some(cid), false),
        fn_item(b, "make_stop", "sig-1", Some(cid), false),
    ];

    store.ingest_nodes(&ks, nodes).expect("ingest nodes");

    let rows = run_query(&store);
    assert_eq!(
        rows.len(),
        1,
        "same-name-duplicate should emit one row; rows={rows:?}"
    );
    assert_eq!(row_str(&rows[0], "cluster_id"), Some(cid));
    assert_eq!(row_str(&rows[0], "a_name"), Some("make_stop"));
    assert_eq!(row_str(&rows[0], "b_name"), Some("make_stop"));
    assert_eq!(row_str(&rows[0], "a_qname"), Some(a));
    assert_eq!(row_str(&rows[0], "b_qname"), Some(b));
}

#[test]
fn synonym_renamed_duplicate_fns_emit_one_cluster_row() {
    let mut store = PetgraphStore::new();
    let ks = keyspace();

    let cid = "sha-synonym-renamed-cluster";
    let a = "crate_a::stop::stop_from_bps";
    let b = "crate_b::risk::stop_from_basis_points";

    let nodes = vec![
        fn_item(a, "stop_from_bps", "sig-2", Some(cid), false),
        fn_item(b, "stop_from_basis_points", "sig-2", Some(cid), false),
    ];

    store.ingest_nodes(&ks, nodes).expect("ingest nodes");

    let rows = run_query(&store);
    assert_eq!(
        rows.len(),
        1,
        "synonym-renamed-duplicate should emit one row even though names differ; rows={rows:?}"
    );
    assert_eq!(row_str(&rows[0], "cluster_id"), Some(cid));
    assert_ne!(
        row_str(&rows[0], "a_name"),
        row_str(&rows[0], "b_name"),
        "synonym-rename scar asserts divergent names — a_name should differ from b_name"
    );
}

#[test]
fn three_crate_fixture_with_two_clusters_emits_both() {
    let mut store = PetgraphStore::new();
    let ks = keyspace();

    let cid_a = "sha-cluster-a";
    let a1 = "crate_1::domain::make_stop";
    let a2 = "crate_2::domain::make_stop";

    let cid_b = "sha-cluster-b";
    let b1 = "crate_2::risk::exposure_from_pct";
    let b2 = "crate_3::risk::exposure_from_percent";

    let c1 = "crate_1::util::format_price";
    let c2 = "crate_2::util::format_qty";
    let c3 = "crate_3::util::render_log";

    let nodes = vec![
        fn_item(a1, "make_stop", "sig-A", Some(cid_a), false),
        fn_item(a2, "make_stop", "sig-A", Some(cid_a), false),
        fn_item(b1, "exposure_from_pct", "sig-B", Some(cid_b), false),
        fn_item(b2, "exposure_from_percent", "sig-B", Some(cid_b), false),
        fn_item(c1, "format_price", "sig-C1", None, false),
        fn_item(c2, "format_qty", "sig-C2", None, false),
        fn_item(c3, "render_log", "sig-C3", None, false),
    ];

    store.ingest_nodes(&ks, nodes).expect("ingest nodes");

    let rows = run_query(&store);
    assert_eq!(
        rows.len(),
        2,
        "3-crate fixture with two clusters should emit exactly 2 rows; rows={rows:?}"
    );

    let cluster_ids: Vec<&str> = rows
        .iter()
        .filter_map(|r| row_str(r, "cluster_id"))
        .collect();
    assert!(
        cluster_ids.contains(&cid_a),
        "Cluster A (same-name) must appear; got {cluster_ids:?}"
    );
    assert!(
        cluster_ids.contains(&cid_b),
        "Cluster B (synonym-renamed) must appear; got {cluster_ids:?}"
    );
}

#[test]
fn unique_signatures_emit_no_rows() {
    let mut store = PetgraphStore::new();
    let ks = keyspace();

    let nodes = vec![
        fn_item("crate_a::f1", "f1", "sig-alpha", None, false),
        fn_item("crate_b::f2", "f2", "sig-beta", None, false),
        fn_item("crate_c::f3", "f3", "sig-gamma", None, false),
    ];

    store.ingest_nodes(&ks, nodes).expect("ingest nodes");

    let rows = run_query(&store);
    assert!(
        rows.is_empty(),
        "unique signatures must produce no HSB clusters; rows={rows:?}"
    );
}

#[test]
fn test_tagged_pair_excluded() {
    let mut store = PetgraphStore::new();
    let ks = keyspace();

    let cid = "sha-test-only-cluster";
    let nodes = vec![
        fn_item(
            "crate_a::tests::make_stop",
            "make_stop",
            "sig-t",
            Some(cid),
            true,
        ),
        fn_item(
            "crate_b::tests::make_stop",
            "make_stop",
            "sig-t",
            Some(cid),
            true,
        ),
    ];

    store.ingest_nodes(&ks, nodes).expect("ingest nodes");

    let rows = run_query(&store);
    assert!(
        rows.is_empty(),
        "pair where both sides are test-scoped must be excluded; rows={rows:?}"
    );
}

#[test]
fn mixed_test_and_production_pair_excluded() {
    let mut store = PetgraphStore::new();
    let ks = keyspace();

    let cid = "sha-mixed-cluster";
    let nodes = vec![
        fn_item("crate_a::make_stop", "make_stop", "sig-m", Some(cid), false),
        fn_item(
            "crate_b::tests::make_stop",
            "make_stop",
            "sig-m",
            Some(cid),
            true,
        ),
    ];

    store.ingest_nodes(&ks, nodes).expect("ingest nodes");

    let rows = run_query(&store);
    assert!(
        rows.is_empty(),
        "prod+test pair must be excluded — is_test=false gates both sides; rows={rows:?}"
    );
}

#[test]
fn same_qname_cross_bin_pair_fires() {
    let mut a = fn_item("tif::helper", "helper", "sig-x", Some("c9"), false);
    a.id = format!("{}#bin:alpha", a.id);
    a.props
        .insert("file".into(), PropValue::Str("src/bin/alpha.rs".into()));
    a.props
        .insert("target".into(), PropValue::Str("bin:alpha".into()));
    let mut b = fn_item("tif::helper", "helper", "sig-x", Some("c9"), false);
    b.id = format!("{}#bin:beta", b.id);
    b.props
        .insert("file".into(), PropValue::Str("src/bin/beta.rs".into()));
    b.props
        .insert("target".into(), PropValue::Str("bin:beta".into()));

    let ks = keyspace();
    let mut store = PetgraphStore::new();
    store.ingest_nodes(&ks, vec![a, b]).expect("ingest");
    let rows = run_query(&store);
    assert_eq!(
        rows.len(),
        1,
        "the same-qname cross-target twin pair must be reported (RFC-054 §3.5.4)"
    );
}

#[test]
fn same_qname_same_file_cross_target_pair_fires() {
    let mut a = fn_item(
        "tif::shared_helper",
        "shared_helper",
        "sig-s",
        Some("c10"),
        false,
    );
    a.id = format!("{}#bin:alpha", a.id);
    a.props
        .insert("file".into(), PropValue::Str("src/shared.rs".into()));
    a.props
        .insert("target".into(), PropValue::Str("bin:alpha".into()));
    let mut b = fn_item(
        "tif::shared_helper",
        "shared_helper",
        "sig-s",
        Some("c10"),
        false,
    );
    b.id = format!("{}#bin:beta", b.id);
    b.props
        .insert("file".into(), PropValue::Str("src/shared.rs".into()));
    b.props
        .insert("target".into(), PropValue::Str("bin:beta".into()));

    let ks = keyspace();
    let mut store = PetgraphStore::new();
    store.ingest_nodes(&ks, vec![a, b]).expect("ingest");
    let rows = run_query(&store);
    assert_eq!(
        rows.len(),
        1,
        "equal-file cross-target twins must still pair (target tie-break)"
    );
}

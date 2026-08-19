use cfdb_core::fact::{Edge, Node, PropValue};
use cfdb_core::result::RowValue;
use cfdb_core::schema::{EdgeLabel, Keyspace, Label};
use cfdb_core::store::{QueryBackend, StoreBackend};
use cfdb_petgraph::{persist, PetgraphStore};

use crate::QueryEngine;

fn ks() -> Keyspace {
    Keyspace::new("edge_match_242")
}

fn parse(cypher: &str) -> cfdb_core::query::Query {
    cfdb_query::parse(cypher).expect("parse")
}

fn scalar_int(row: &cfdb_core::result::Row, key: &str) -> i64 {
    match row.get(key).expect("column present") {
        RowValue::Scalar(PropValue::Int(n)) => *n,
        other => panic!("expected Int, got {:?}", other),
    }
}

fn scalar_str(row: &cfdb_core::result::Row, key: &str) -> String {
    match row.get(key).expect("column present") {
        RowValue::Scalar(PropValue::Str(s)) => s.clone(),
        other => panic!("expected Str, got {:?}", other),
    }
}

fn minimal_fixture() -> (Vec<Node>, Vec<Edge>) {
    let nodes = vec![
        Node::new("nA", Label::new("N")).with_prop("qname", "alpha"),
        Node::new("nB", Label::new("N")).with_prop("qname", "beta"),
    ];
    let edges =
        vec![Edge::new("nA", "nB", EdgeLabel::new("REL")).with_prop("weight", PropValue::Int(7))];
    (nodes, edges)
}

fn fresh_store_with_fixture() -> (PetgraphStore, Keyspace) {
    let (nodes, edges) = minimal_fixture();
    let mut store = PetgraphStore::new();
    let k = ks();
    store.ingest_nodes(&k, nodes).expect("ingest nodes");
    store.ingest_edges(&k, edges).expect("ingest edges");
    (store, k)
}

#[test]
fn count_named_edge_var_anonymous_label() {
    let (store, k) = fresh_store_with_fixture();
    let q = parse("MATCH (a)-[r]->(b) RETURN count(r)");
    let r = QueryEngine::new(&store).execute(&k, &q).expect("exec");
    assert_eq!(
        scalar_int(&r.rows[0], "count"),
        1,
        "MATCH (a)-[r]->(b) RETURN count(r) on 1-edge fixture must equal 1; rows={:?}",
        r.rows
    );
}

#[test]
fn count_named_edge_var_with_label() {
    let (store, k) = fresh_store_with_fixture();
    let q = parse("MATCH (a)-[r:REL]->(b) RETURN count(r)");
    let r = QueryEngine::new(&store).execute(&k, &q).expect("exec");
    assert_eq!(scalar_int(&r.rows[0], "count"), 1);
}

#[test]
fn count_named_edge_var_with_typed_endpoints() {
    let (store, k) = fresh_store_with_fixture();
    let q = parse("MATCH (a:N)-[r:REL]->(b:N) RETURN count(r)");
    let r = QueryEngine::new(&store).execute(&k, &q).expect("exec");
    assert_eq!(scalar_int(&r.rows[0], "count"), 1);
}

#[test]
fn count_star_also_works_as_control() {
    let (store, k) = fresh_store_with_fixture();
    let q = parse("MATCH (a)-[r]->(b) RETURN count(*)");
    let r = QueryEngine::new(&store).execute(&k, &q).expect("exec");
    assert_eq!(scalar_int(&r.rows[0], "count"), 1);
}

#[test]
fn count_from_node_var_also_works_as_control() {
    let (store, k) = fresh_store_with_fixture();
    let q = parse("MATCH (a)-[r]->(b) RETURN count(a)");
    let r = QueryEngine::new(&store).execute(&k, &q).expect("exec");
    assert_eq!(scalar_int(&r.rows[0], "count"), 1);
}

#[test]
fn edge_var_property_access_label() {
    let (store, k) = fresh_store_with_fixture();
    let q = parse("MATCH (a)-[r:REL]->(b) RETURN r.label");
    let r = QueryEngine::new(&store).execute(&k, &q).expect("exec");
    assert_eq!(scalar_str(&r.rows[0], "r.label"), "REL");
}

#[test]
fn edge_var_property_access_custom_prop() {
    let (store, k) = fresh_store_with_fixture();
    let q = parse("MATCH (a)-[r:REL]->(b) RETURN r.weight");
    let r = QueryEngine::new(&store).execute(&k, &q).expect("exec");
    assert_eq!(scalar_int(&r.rows[0], "r.weight"), 7);
}

#[test]
fn count_chained_edges() {
    let mut store = PetgraphStore::new();
    let k = ks();
    store
        .ingest_nodes(
            &k,
            vec![
                Node::new("a", Label::new("N")),
                Node::new("b", Label::new("N")),
                Node::new("c", Label::new("N")),
            ],
        )
        .expect("ingest nodes");
    store
        .ingest_edges(
            &k,
            vec![
                Edge::new("a", "b", EdgeLabel::new("R1")),
                Edge::new("b", "c", EdgeLabel::new("R2")),
            ],
        )
        .expect("ingest edges");

    let q = parse("MATCH (a)-[r1:R1]->(b), (b)-[r2:R2]->(c) RETURN count(r1)");
    let r = QueryEngine::new(&store).execute(&k, &q).expect("exec");
    assert_eq!(scalar_int(&r.rows[0], "count"), 1);

    let q2 = parse("MATCH (a)-[r1:R1]->(b), (b)-[r2:R2]->(c) RETURN count(r2)");
    let r2 = QueryEngine::new(&store).execute(&k, &q2).expect("exec");
    assert_eq!(scalar_int(&r2.rows[0], "count"), 1);
}

fn roundtripped_store() -> (PetgraphStore, Keyspace, tempfile::TempDir) {
    let (source, k) = fresh_store_with_fixture();
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("edge_match_242.json");
    persist::save(&source, &k, &path).expect("save");

    let mut dest = PetgraphStore::new();
    persist::load(&mut dest, &k, &path).expect("load");
    (dest, k, dir)
}

#[test]
fn roundtrip_count_named_edge_var() {
    let (store, k, _dir) = roundtripped_store();
    let q = parse("MATCH (a)-[r:REL]->(b) RETURN count(r)");
    let r = QueryEngine::new(&store).execute(&k, &q).expect("exec");
    assert_eq!(
        scalar_int(&r.rows[0], "count"),
        1,
        "persist-roundtrip MATCH ...-[r:REL]->... count(r) must equal 1; rows={:?}",
        r.rows
    );
}

#[test]
fn roundtrip_edge_property_access() {
    let (store, k, _dir) = roundtripped_store();
    let q = parse("MATCH (a)-[r:REL]->(b) RETURN r.label");
    let r = QueryEngine::new(&store).execute(&k, &q).expect("exec");
    assert_eq!(scalar_str(&r.rows[0], "r.label"), "REL");
}

#[test]
fn parallel_edges_each_produce_a_row() {
    let mut store = PetgraphStore::new();
    let k = ks();
    store
        .ingest_nodes(
            &k,
            vec![
                Node::new("a", Label::new("N")),
                Node::new("b", Label::new("N")),
            ],
        )
        .expect("ingest nodes");
    store
        .ingest_edges(
            &k,
            vec![
                Edge::new("a", "b", EdgeLabel::new("REL")),
                Edge::new("a", "b", EdgeLabel::new("REL")),
                Edge::new("a", "b", EdgeLabel::new("REL")),
            ],
        )
        .expect("ingest edges");

    let q = parse("MATCH (a)-[r:REL]->(b) RETURN count(r)");
    let r = QueryEngine::new(&store).execute(&k, &q).expect("exec");
    assert_eq!(
        scalar_int(&r.rows[0], "count"),
        3,
        "3 parallel edges must each produce a row; rows={:?}",
        r.rows
    );
}

#[test]
fn anonymous_edge_pattern_still_works() {
    let (store, k) = fresh_store_with_fixture();

    let q = parse("MATCH (a:N)-[:REL]->(b:N) RETURN count(a)");
    let r = QueryEngine::new(&store).execute(&k, &q).expect("exec");
    assert_eq!(scalar_int(&r.rows[0], "count"), 1);
}

#[test]
fn unknown_edge_label_still_warns() {
    let (store, k) = fresh_store_with_fixture();
    let q = parse("MATCH (a)-[r:NOSUCH]->(b) RETURN count(r)");
    let r = QueryEngine::new(&store).execute(&k, &q).expect("exec");
    assert!(
        r.warnings
            .iter()
            .any(|w| matches!(w.kind, cfdb_core::result::WarningKind::UnknownEdgeLabel)),
        "UnknownEdgeLabel warning must fire for MATCH (a)-[r:NOSUCH]->(b); warnings={:?}",
        r.warnings
    );
}

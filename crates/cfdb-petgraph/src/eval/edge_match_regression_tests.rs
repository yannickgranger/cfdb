//! Regression tests for issue #242 — edge-pattern MATCH returns 0 rows.
//!
//! Root cause (confirmed via probe 2026-04-24): `build_path_binding`
//! (`pattern.rs:260`) inserts `pp.from.var` and `pp.to.var` as
//! `Binding::NodeRef` but does not insert `pp.edge.var`. The `Binding`
//! enum lacked an `EdgeRef` variant, making the edge variable
//! architecturally unrepresentable — every `count(r)` or `r.prop`
//! evaluated against an empty binding and returned Null.
//!
//! # Test shapes
//!
//! **Programmatic fixtures** (control): build a `PetgraphStore`
//! in-process, ingest 2 nodes + 1 edge, run the failing queries.
//!
//! **Persist-roundtrip fixtures** (the shape the reported bug lives
//! on): write the same fixture to JSON via `persist::save`, load it
//! into a fresh store via `persist::load`, run the same queries.
//!
//! Both shapes FAIL on develop tip `346eab1` and PASS on the fix.
//! Per repo `CLAUDE.md §2.5`, the red→green transition in a single
//! PR IS the bug-fix regression contract.

use std::collections::BTreeMap;

use cfdb_core::fact::{Edge, Node, PropValue};
use cfdb_core::result::RowValue;
use cfdb_core::schema::{EdgeLabel, Keyspace, Label};
use cfdb_core::store::StoreBackend;

use crate::PetgraphStore;

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

/// Build the minimal 2-node, 1-edge fixture used by programmatic tests.
fn minimal_fixture() -> (Vec<Node>, Vec<Edge>) {
    let nodes = vec![
        Node::new("nA", Label::new("N")).with_prop("qname", "alpha"),
        Node::new("nB", Label::new("N")).with_prop("qname", "beta"),
    ];
    let edges = vec![Edge::new("nA", "nB", EdgeLabel::new("REL"))
        .with_prop("weight", PropValue::Int(7))];
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

// ---------- Programmatic fixtures ----------

#[test]
fn count_named_edge_var_anonymous_label() {
    let (store, k) = fresh_store_with_fixture();
    let q = parse("MATCH (a)-[r]->(b) RETURN count(r)");
    let r = store.execute(&k, &q).expect("exec");
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
    let r = store.execute(&k, &q).expect("exec");
    assert_eq!(scalar_int(&r.rows[0], "count"), 1);
}

#[test]
fn count_named_edge_var_with_typed_endpoints() {
    let (store, k) = fresh_store_with_fixture();
    let q = parse("MATCH (a:N)-[r:REL]->(b:N) RETURN count(r)");
    let r = store.execute(&k, &q).expect("exec");
    assert_eq!(scalar_int(&r.rows[0], "count"), 1);
}

#[test]
fn count_star_also_works_as_control() {
    // CountStar doesn't depend on edge-var binding; serves as a control
    // that edge-traversal itself is finding the pair.
    let (store, k) = fresh_store_with_fixture();
    let q = parse("MATCH (a)-[r]->(b) RETURN count(*)");
    let r = store.execute(&k, &q).expect("exec");
    assert_eq!(scalar_int(&r.rows[0], "count"), 1);
}

#[test]
fn count_from_node_var_also_works_as_control() {
    // count(a) on the from-node already works pre-fix; control that the
    // fix doesn't regress it.
    let (store, k) = fresh_store_with_fixture();
    let q = parse("MATCH (a)-[r]->(b) RETURN count(a)");
    let r = store.execute(&k, &q).expect("exec");
    assert_eq!(scalar_int(&r.rows[0], "count"), 1);
}

#[test]
fn edge_var_property_access_label() {
    let (store, k) = fresh_store_with_fixture();
    let q = parse("MATCH (a)-[r:REL]->(b) RETURN r.label");
    let r = store.execute(&k, &q).expect("exec");
    assert_eq!(scalar_str(&r.rows[0], "r.label"), "REL");
}

#[test]
fn edge_var_property_access_custom_prop() {
    let (store, k) = fresh_store_with_fixture();
    let q = parse("MATCH (a)-[r:REL]->(b) RETURN r.weight");
    let r = store.execute(&k, &q).expect("exec");
    assert_eq!(scalar_int(&r.rows[0], "r.weight"), 7);
}

// ---------- Chained patterns ----------

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
    let r = store.execute(&k, &q).expect("exec");
    assert_eq!(scalar_int(&r.rows[0], "count"), 1);

    let q2 = parse("MATCH (a)-[r1:R1]->(b), (b)-[r2:R2]->(c) RETURN count(r2)");
    let r2 = store.execute(&k, &q2).expect("exec");
    assert_eq!(scalar_int(&r2.rows[0], "count"), 1);
}

// ---------- Persist-roundtrip fixtures ----------

fn roundtripped_store() -> (PetgraphStore, Keyspace, tempfile::TempDir) {
    let (source, k) = fresh_store_with_fixture();
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("edge_match_242.json");
    crate::persist::save(&source, &k, &path).expect("save");

    let mut dest = PetgraphStore::new();
    crate::persist::load(&mut dest, &k, &path).expect("load");
    (dest, k, dir)
}

#[test]
fn roundtrip_count_named_edge_var() {
    let (store, k, _dir) = roundtripped_store();
    let q = parse("MATCH (a)-[r:REL]->(b) RETURN count(r)");
    let r = store.execute(&k, &q).expect("exec");
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
    let r = store.execute(&k, &q).expect("exec");
    assert_eq!(scalar_str(&r.rows[0], "r.label"), "REL");
}

// ---------- Invariant: anonymous edges still work (regression guard) ----------

#[test]
fn anonymous_edge_pattern_still_works() {
    // All 31 shipped .cypher rules use anonymous -[:LABEL]-> patterns.
    // The fix must not break them.
    let (store, k) = fresh_store_with_fixture();

    let q = parse("MATCH (a:N)-[:REL]->(b:N) RETURN count(a)");
    let r = store.execute(&k, &q).expect("exec");
    assert_eq!(scalar_int(&r.rows[0], "count"), 1);
}

#[test]
fn unknown_edge_label_still_warns() {
    // Label validation must still fire UnknownEdgeLabel when the label
    // is absent. The fix must not silence this signal.
    //
    // Note: when the stream is empty (which is what
    // `warn_on_unknown_edge_label` triggers by returning `iter::empty`),
    // `group_and_aggregate` on an empty table produces no rows. That is
    // pre-existing behavior unrelated to this bug — this test asserts
    // only on the WARNING, not the row shape.
    let (store, k) = fresh_store_with_fixture();
    let q = parse("MATCH (a)-[r:NOSUCH]->(b) RETURN count(r)");
    let r = store.execute(&k, &q).expect("exec");
    assert!(
        r.warnings.iter().any(|w| matches!(
            w.kind,
            cfdb_core::result::WarningKind::UnknownEdgeLabel
        )),
        "UnknownEdgeLabel warning must fire for MATCH (a)-[r:NOSUCH]->(b); warnings={:?}",
        r.warnings
    );
}

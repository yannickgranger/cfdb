//! Acceptance: the 9 canonical F-capability test queries from
//! `.concept-graph/studies/001-graph-store-selection-methodology.md` §4.1
//! must all parse into a `Query` AST.
//!
//! Each test additionally performs a serde roundtrip (JSON) to validate that
//! the AST carries enough information to reconstruct the query and that every
//! variant is wired up in Serialize/Deserialize.

use cfdb_core::{Aggregation, Pattern, Predicate, ProjectionValue, Query};
use cfdb_query::parse;

fn parse_and_roundtrip(label: &str, src: &str) -> Query {
    let q = match parse(src) {
        Ok(q) => q,
        Err(e) => panic!("{label} failed to parse: {e}\nquery:\n{src}"),
    };
    let json = serde_json::to_string(&q).expect("serialize");
    let back: Query = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(q, back, "{label} serde roundtrip mismatch");
    q
}

// F1 — Fixed-hop label + property match (aggregation form, per study 001 §4.2).
// The Cartesian form is the F1a footgun flagged by the shape lint; the
// aggregation form is what tools should actually emit.
#[test]
fn f1_fixed_hop_label_property_match() {
    let q = parse_and_roundtrip(
        "F1",
        r#"
        MATCH (a:Item)
        WITH regexp_extract(a.qname, '[^:]+$') AS name,
             collect(DISTINCT a.crate) AS crates
        WHERE size(crates) > 1
        RETURN count(*) AS n
        "#,
    );
    assert_eq!(q.match_clauses.len(), 1);
    assert!(q.with_clause.is_some());
}

// F2 — Variable-length path [:CALLS*1..10].
#[test]
fn f2_variable_length_path() {
    let q = parse_and_roundtrip(
        "F2",
        "MATCH (ep:EntryPoint)-[:CALLS*1..10]->(fn:Item) RETURN fn",
    );
    assert_eq!(q.match_clauses.len(), 1);
    match &q.match_clauses[0] {
        Pattern::Path(p) => {
            assert_eq!(p.edge.var_length, Some((1, 10)));
        }
        other => panic!("expected Path, got {other:?}"),
    }
}

// F3 — Property regex in WHERE (=~).
#[test]
fn f3_property_regex_in_where() {
    let q = parse_and_roundtrip(
        "F3",
        "MATCH (callee:Item) WHERE callee.qname =~ 'chrono::Utc::now' RETURN callee",
    );
    match q.where_clause.as_ref().expect("where clause") {
        Predicate::Regex { .. } => {}
        other => panic!("expected Regex, got {other:?}"),
    }
}

// F4 — OPTIONAL MATCH / left join.
#[test]
fn f4_optional_match() {
    let q = parse_and_roundtrip(
        "F4",
        "MATCH (c:Concept) OPTIONAL MATCH (canonical:Item)-[:CANONICAL_FOR]->(c) RETURN c, canonical",
    );
    assert!(q
        .match_clauses
        .iter()
        .any(|p| matches!(p, Pattern::Optional(_))));
}

// F5 — External parameter sets / input bucket joins.
#[test]
fn f5_param_list_in() {
    let q = parse_and_roundtrip(
        "F5",
        "UNWIND $plan_drop AS drop MATCH (i:Item) WHERE i.qname IN $plan_drop RETURN i",
    );
    assert!(q
        .match_clauses
        .iter()
        .any(|p| matches!(p, Pattern::Unwind { .. })));
    match q.where_clause.as_ref().expect("where clause") {
        Predicate::In { .. } => {}
        other => panic!("expected In, got {other:?}"),
    }
}

// F6 — NOT EXISTS / anti-join.
#[test]
fn f6_not_exists_anti_join() {
    let q = parse_and_roundtrip(
        "F6",
        "MATCH (i:Item) WHERE NOT EXISTS { MATCH (i)-[:CALLS]->(fallback:Item) } RETURN i",
    );
    match q.where_clause.as_ref().expect("where clause") {
        Predicate::NotExists { .. } => {}
        other => panic!("expected NotExists, got {other:?}"),
    }
}

// F7 — Aggregation + grouping (count + group by implicit projection).
#[test]
fn f7_aggregation_grouping() {
    let q = parse_and_roundtrip(
        "F7",
        "MATCH (i:Item) WITH i.crate AS crate, count(*) AS n RETURN crate, n",
    );
    let with = q.with_clause.as_ref().expect("with clause");
    assert_eq!(with.projections.len(), 2);
    assert!(with.projections.iter().any(|p| matches!(
        p.value,
        ProjectionValue::Aggregation(Aggregation::CountStar)
    )));
}

// F8 — Parameterized queries — $qname, $rule_path bound safely.
#[test]
fn f8_parameterized_query() {
    let q = parse_and_roundtrip("F8", "MATCH (i:Item) WHERE i.qname = $qname RETURN i");
    match q.where_clause.as_ref().expect("where clause") {
        Predicate::Compare { right, .. } => {
            assert!(matches!(right, cfdb_core::Expr::Param(_)));
        }
        other => panic!("expected Compare, got {other:?}"),
    }
}

// F9 — Multi-valued repeated edges between same pair (bag semantics). Here
// we just need to parse a CALLS edge pattern with a bound edge variable so
// callers can distinguish call sites via edge properties.
#[test]
fn f9_multi_valued_edges_with_var() {
    let q = parse_and_roundtrip("F9", "MATCH (a:Item)-[c:CALLS]->(b:Item) RETURN a, c, b");
    match &q.match_clauses[0] {
        Pattern::Path(p) => {
            assert_eq!(p.edge.var.as_deref(), Some("c"));
        }
        other => panic!("expected Path, got {other:?}"),
    }
}

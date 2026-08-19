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

#[test]
fn open_range_variable_length_path_parses_to_u32_max() {
    let q = parse_and_roundtrip(
        "open-range",
        "MATCH (a:Item)<-[:CALLS*1..]-(b:Item) RETURN a",
    );
    assert_eq!(q.match_clauses.len(), 1);
    match &q.match_clauses[0] {
        Pattern::Path(p) => {
            assert_eq!(
                p.edge.var_length,
                Some((1, u32::MAX)),
                "an omitted upper bound `*1..` must parse to the open-range sentinel u32::MAX",
            );
        }
        other => panic!("expected Path, got {other:?}"),
    }

    let closed = parse_and_roundtrip(
        "closed-range-regression",
        "MATCH (a:Item)-[:CALLS*2..7]->(b:Item) RETURN a",
    );
    match &closed.match_clauses[0] {
        Pattern::Path(p) => assert_eq!(p.edge.var_length, Some((2, 7))),
        other => panic!("expected Path, got {other:?}"),
    }
}

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

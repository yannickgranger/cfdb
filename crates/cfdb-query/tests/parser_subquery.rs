use cfdb_core::{Predicate, Query};
use cfdb_query::parse;

fn parse_or_panic(label: &str, src: &str) -> Query {
    match parse(src) {
        Ok(q) => q,
        Err(e) => panic!("{label} failed to parse: {e}\nquery:\n{src}"),
    }
}

fn not_exists_inner(q: &Query) -> &Predicate {
    let outer = q.where_clause.as_ref().expect("outer WHERE clause");
    let Predicate::NotExists { inner } = outer else {
        panic!("expected outer Predicate::NotExists, got {outer:?}");
    };
    let inner_q = inner.as_ref();
    inner_q
        .where_clause
        .as_ref()
        .expect("inner subquery WHERE clause")
}

#[test]
fn parse_not_exists_with_regex_predicate() {
    let q = parse_or_panic(
        "regex inside subquery",
        "MATCH (a:Item) WHERE NOT EXISTS { MATCH (a)-[:CALLS]->(b:Item) WHERE b.qname =~ 'foo.*' } RETURN a",
    );
    match not_exists_inner(&q) {
        Predicate::Regex { .. } => {}
        other => panic!("expected inner Regex, got {other:?}"),
    }
}

#[test]
fn parse_not_exists_with_in_predicate() {
    let q = parse_or_panic(
        "IN inside subquery",
        "MATCH (a:Item) WHERE NOT EXISTS { MATCH (a)-[:CALLS]->(b:Item) WHERE b.qname IN $allowlist } RETURN a",
    );
    match not_exists_inner(&q) {
        Predicate::In { .. } => {}
        other => panic!("expected inner In, got {other:?}"),
    }
}

#[test]
fn parse_not_exists_with_and_predicate() {
    let q = parse_or_panic(
        "AND inside subquery",
        "MATCH (a:Item) WHERE NOT EXISTS { MATCH (a)-[:CALLS]->(b:Item) WHERE b.crate = 'std' AND b.visibility = 'pub' } RETURN a",
    );
    match not_exists_inner(&q) {
        Predicate::And(_, _) => {}
        other => panic!("expected inner And, got {other:?}"),
    }
}

#[test]
fn parse_not_exists_with_or_predicate() {
    let q = parse_or_panic(
        "OR inside subquery",
        "MATCH (a:Item) WHERE NOT EXISTS { MATCH (a)-[:CALLS]->(b:Item) WHERE b.crate = 'std' OR b.crate = 'core' } RETURN a",
    );
    match not_exists_inner(&q) {
        Predicate::Or(_, _) => {}
        other => panic!("expected inner Or, got {other:?}"),
    }
}

#[test]
fn parse_not_exists_with_not_predicate() {
    let q = parse_or_panic(
        "NOT inside subquery",
        "MATCH (a:Item) WHERE NOT EXISTS { MATCH (a)-[:CALLS]->(b:Item) WHERE NOT b.crate = 'std' } RETURN a",
    );
    match not_exists_inner(&q) {
        Predicate::Not(_) => {}
        other => panic!("expected inner Not, got {other:?}"),
    }
}

#[test]
fn parse_not_exists_with_nested_not_exists() {
    let q = parse_or_panic(
        "nested NOT EXISTS",
        "MATCH (a:Item) WHERE NOT EXISTS { MATCH (a)-[:CALLS]->(b:Item) WHERE NOT EXISTS { MATCH (b)-[:CALLS]->(c:Item) } } RETURN a",
    );
    match not_exists_inner(&q) {
        Predicate::NotExists { .. } => {}
        other => panic!("expected inner NotExists (nested), got {other:?}"),
    }
}

#[test]
fn parse_not_exists_no_inner_where_unchanged() {
    let q = parse_or_panic(
        "no inner WHERE",
        "MATCH (i:Item) WHERE NOT EXISTS { MATCH (i)-[:CALLS]->(fallback:Item) } RETURN i",
    );
    let outer = q.where_clause.as_ref().expect("outer WHERE");
    let Predicate::NotExists { inner } = outer else {
        panic!("expected NotExists, got {outer:?}");
    };
    assert!(
        inner.where_clause.is_none(),
        "inner WHERE must remain None when no `WHERE` clause is present"
    );
}

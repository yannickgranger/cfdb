//! Subquery WHERE grammar — `NOT EXISTS { MATCH ... WHERE ... }` bodies.
//!
//! Audit 2026-W17 / EPIC #273 (CFDB-QRY-H1, #271): `subquery_parser` used to
//! re-implement a stripped-down `Compare`/`Ne`-only predicate grammar inline,
//! split-brain with the outer `predicate_parser`. NOT EXISTS bodies could not
//! use `=~` / `IN` / `AND` / `OR` / inner `NOT` / nested `NOT EXISTS` even
//! though the outer WHERE supported all of them.
//!
//! These tests pin the post-fix behavior: the inner WHERE accepts the full
//! v0.1 predicate subset, exercised by an end-to-end parse + AST shape check.
//! The single-resolution-point property (RFC-035 §3.3 invariant-owner pattern)
//! means any future extension to the outer predicate grammar lights these
//! cases up automatically.

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

/// Pre-fix baseline preserved: the simplest `NOT EXISTS { MATCH ... }` body
/// (no inner WHERE) still parses to a `NotExists` with no inner WHERE clause.
/// This is the F6 case that was always supported.
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

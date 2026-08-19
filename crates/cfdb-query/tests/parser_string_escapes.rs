use cfdb_core::{Expr, Predicate, PropValue};
use cfdb_query::parse;

fn literal_string_from_eq(query: &str) -> String {
    let q = parse(query).unwrap_or_else(|e| panic!("parse failed for {query:?}: {e}"));
    let pred = q
        .where_clause
        .as_ref()
        .unwrap_or_else(|| panic!("expected WHERE clause for {query:?}"));
    match pred {
        Predicate::Compare { right, .. } => match right {
            Expr::Literal(PropValue::Str(s)) => s.clone(),
            other => panic!("expected Literal(Str), got {other:?}"),
        },
        other => panic!("expected Compare, got {other:?}"),
    }
}

#[test]
fn single_quoted_with_escaped_apostrophe() {
    let s = literal_string_from_eq(r#"MATCH (n:Item) WHERE n.qname = 'don\'t' RETURN n"#);
    assert_eq!(s, "don't");
    assert_eq!(s.len(), 5);
}

#[test]
fn double_quoted_with_escaped_double_quote() {
    let s = literal_string_from_eq(r#"MATCH (n:Item) WHERE n.qname = "say \"hi\"" RETURN n"#);
    assert_eq!(s, r#"say "hi""#);
}

#[test]
fn newline_escape_yields_lf_byte() {
    let s = literal_string_from_eq(r#"MATCH (n:Item) WHERE n.qname = 'line1\nline2' RETURN n"#);
    assert_eq!(s, "line1\nline2");
    assert!(s.contains('\n'));
}

#[test]
fn carriage_return_escape_yields_cr_byte() {
    let s = literal_string_from_eq(r#"MATCH (n:Item) WHERE n.qname = 'a\rb' RETURN n"#);
    assert_eq!(s, "a\rb");
}

#[test]
fn tab_escape_yields_ht_byte() {
    let s = literal_string_from_eq(r#"MATCH (n:Item) WHERE n.qname = 'a\tb' RETURN n"#);
    assert_eq!(s, "a\tb");
}

#[test]
fn backslash_escape_yields_single_backslash() {
    let s = literal_string_from_eq(r#"MATCH (n:Item) WHERE n.qname = 'a\\b' RETURN n"#);
    assert_eq!(s, "a\\b");
    assert_eq!(s.len(), 3);
}

#[test]
fn double_quote_inside_single_quoted_string_unchanged() {
    let s = literal_string_from_eq(r#"MATCH (n:Item) WHERE n.qname = 'a"b' RETURN n"#);
    assert_eq!(s, r#"a"b"#);
}

#[test]
fn single_quote_inside_double_quoted_string_unchanged() {
    let s = literal_string_from_eq(r#"MATCH (n:Item) WHERE n.qname = "a'b" RETURN n"#);
    assert_eq!(s, "a'b");
}

#[test]
fn unsupported_escape_rejects_with_message() {
    let q = r#"MATCH (n:Item) WHERE n.qname = 'a\zb' RETURN n"#;
    let err = parse(q).expect_err("\\z is not a supported escape");
    let msg = err.to_string();
    assert!(
        msg.contains("\\z"),
        "error message should name the offending escape, got: {msg}"
    );
    assert!(
        msg.contains("supported"),
        "error message should list the supported set, got: {msg}"
    );
}

#[test]
fn backslash_followed_by_closing_quote_consumes_the_quote() {
    let q = r#"MATCH (n:Item) WHERE n.qname = 'a\' RETURN n"#;
    assert!(
        parse(q).is_err(),
        "string ending mid-escape must be a parse error"
    );
}

#[test]
fn unescaped_strings_still_parse() {
    let s = literal_string_from_eq(r#"MATCH (n:Item) WHERE n.qname = "hello" RETURN n"#);
    assert_eq!(s, "hello");

    let s = literal_string_from_eq(r#"MATCH (n:Item) WHERE n.qname = 'hello' RETURN n"#);
    assert_eq!(s, "hello");
}

#[test]
fn empty_string_literal_still_parses() {
    let s = literal_string_from_eq(r#"MATCH (n:Item) WHERE n.qname = '' RETURN n"#);
    assert_eq!(s, "");
    let s = literal_string_from_eq(r#"MATCH (n:Item) WHERE n.qname = "" RETURN n"#);
    assert_eq!(s, "");
}

#[test]
fn parse_with_escape_equals_builder_with_raw_value() {
    use cfdb_core::Label;
    use cfdb_query::QueryBuilder;

    let parsed = parse(r#"MATCH (n:Item) WHERE n.qname = 'don\'t' RETURN n"#).expect("parse");
    let built = QueryBuilder::new()
        .match_node("n", Label::new(Label::ITEM))
        .where_eq(
            Expr::Property {
                var: "n".into(),
                prop: "qname".into(),
            },
            Expr::Literal(PropValue::Str("don't".into())),
        )
        .return_items(vec![cfdb_core::Projection {
            value: cfdb_core::ProjectionValue::Expr(Expr::Var("n".into())),
            alias: None,
        }])
        .build();
    assert_eq!(parsed, built);
}

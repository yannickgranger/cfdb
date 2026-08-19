use cfdb_query::{parse, ParseError};

fn unwrap_syntax(err: ParseError) -> (u32, u32, String, Option<String>) {
    let ParseError::Syntax {
        line,
        col,
        message,
        suggestion,
    } = err;
    (line, col, message, suggestion)
}

#[test]
fn create_rejected_with_suggestion() {
    let err = parse("CREATE (a:Item) RETURN a").unwrap_err();
    let (line, col, msg, suggestion) = unwrap_syntax(err);
    assert_eq!(line, 1);
    assert_eq!(col, 1);
    assert!(msg.contains("CREATE"), "msg: {msg}");
    assert!(
        suggestion.as_deref().unwrap_or("").contains("read-only"),
        "suggestion: {suggestion:?}"
    );
}

#[test]
fn merge_rejected_with_suggestion() {
    let err = parse("MERGE (a:Item) RETURN a").unwrap_err();
    let (line, col, msg, suggestion) = unwrap_syntax(err);
    assert_eq!(line, 1);
    assert_eq!(col, 1);
    assert!(msg.contains("MERGE"), "msg: {msg}");
    assert!(
        suggestion.as_deref().unwrap_or("").contains("read-only"),
        "suggestion: {suggestion:?}"
    );
}

#[test]
fn delete_rejected() {
    let err = parse("MATCH (a:Item) DELETE a").unwrap_err();
    let (_, _, msg, suggestion) = unwrap_syntax(err);
    assert!(msg.contains("DELETE"), "msg: {msg}");
    assert!(
        suggestion.as_deref().unwrap_or("").contains("read-only"),
        "suggestion: {suggestion:?}"
    );
}

#[test]
fn set_rejected_with_suggestion() {
    let err = parse("SET n.x = 1").unwrap_err();
    let (line, col, msg, suggestion) = unwrap_syntax(err);
    assert_eq!(line, 1);
    assert_eq!(col, 1);
    assert!(msg.contains("SET"), "msg: {msg}");
    assert!(
        suggestion.as_deref().unwrap_or("").contains("read-only"),
        "suggestion: {suggestion:?}"
    );
}

#[test]
fn remove_rejected_with_suggestion() {
    let err = parse("REMOVE n.x").unwrap_err();
    let (line, col, msg, suggestion) = unwrap_syntax(err);
    assert_eq!(line, 1);
    assert_eq!(col, 1);
    assert!(msg.contains("REMOVE"), "msg: {msg}");
    assert!(
        suggestion.as_deref().unwrap_or("").contains("read-only"),
        "suggestion: {suggestion:?}"
    );
}

#[test]
fn call_rejected_with_suggestion() {
    let err = parse("CALL foo()").unwrap_err();
    let (line, col, msg, suggestion) = unwrap_syntax(err);
    assert_eq!(line, 1);
    assert_eq!(col, 1);
    assert!(msg.contains("CALL"), "msg: {msg}");
    assert!(
        suggestion.as_deref().unwrap_or("").contains("read-only"),
        "suggestion: {suggestion:?}"
    );
}

#[test]
fn detach_rejected_with_suggestion() {
    let err = parse("DETACH (n)").unwrap_err();
    let (line, col, msg, suggestion) = unwrap_syntax(err);
    assert_eq!(line, 1);
    assert_eq!(col, 1);
    assert!(msg.contains("DETACH"), "msg: {msg}");
    assert!(
        suggestion.as_deref().unwrap_or("").contains("read-only"),
        "suggestion: {suggestion:?}"
    );
}

#[test]
fn detach_delete_rejects_on_delete_first() {
    let err = parse("DETACH DELETE n").unwrap_err();
    let (line, col, msg, _) = unwrap_syntax(err);
    assert_eq!(line, 1);
    assert_eq!(col, 8);
    assert!(msg.contains("DELETE"), "msg: {msg}");
}

#[test]
fn multi_statement_rejected() {
    let err = parse("MATCH (a:Item) RETURN a; MATCH (b:Item) RETURN b").unwrap_err();
    let (_, _, msg, suggestion) = unwrap_syntax(err);
    assert!(msg.contains("multi-statement"));
    assert!(suggestion
        .as_deref()
        .unwrap_or("")
        .contains("one statement"));
}

#[test]
fn trailing_semicolon_rejected() {
    let err = parse("MATCH (n:Item) RETURN n;").unwrap_err();
    let (line, col, msg, _) = unwrap_syntax(err);
    assert_eq!(line, 1);
    assert_eq!(col, 24);
    assert!(msg.contains("multi-statement"), "msg: {msg}");
}

#[test]
fn leading_semicolon_rejected() {
    let err = parse(";MATCH (n:Item) RETURN n").unwrap_err();
    let (line, col, msg, _) = unwrap_syntax(err);
    assert_eq!(line, 1);
    assert_eq!(col, 1);
    assert!(msg.contains("multi-statement"), "msg: {msg}");
}

#[test]
fn unterminated_single_quoted_string_rejects() {
    let err = parse("MATCH (n:Item) WHERE n.x = 'hello RETURN n").unwrap_err();
    let (_, _, _msg, _) = unwrap_syntax(err);
}

#[test]
fn unterminated_double_quoted_string_rejects() {
    let err = parse(r#"MATCH (n:Item) WHERE n.x = "hello RETURN n"#).unwrap_err();
    let _ = unwrap_syntax(err);
}

#[test]
fn mid_escape_eof_rejects() {
    let err = parse(r#"MATCH (n:Item) WHERE n.x = 'a\"#).unwrap_err();
    let _ = unwrap_syntax(err);
}

#[test]
fn unsupported_escape_z_rejects_and_lists_supported() {
    let err = parse(r#"MATCH (n:Item) WHERE n.x = 'a\z' RETURN n"#).unwrap_err();
    let (_, _, msg, _) = unwrap_syntax(err);
    assert!(
        msg.contains("\\z") || msg.contains("unsupported escape"),
        "msg should name \\z or 'unsupported escape': {msg}"
    );
}

#[test]
fn unsupported_escape_double_quoted_rejects() {
    let err = parse(r#"MATCH (n:Item) WHERE n.x = "a\q" RETURN n"#).unwrap_err();
    let (_, _, msg, _) = unwrap_syntax(err);
    assert!(
        msg.contains("\\q") || msg.contains("unsupported escape"),
        "msg should name \\q or 'unsupported escape': {msg}"
    );
}

#[test]
fn string_spanning_to_eof_rejects() {
    let err = parse(r#"MATCH (n:Item) WHERE n.x = "foo\nbar"#).unwrap_err();
    let _ = unwrap_syntax(err);
}

#[test]
fn mismatched_quote_kinds_rejects() {
    let err = parse(r#"MATCH (n:Item) WHERE n.x = 'foo" RETURN n"#).unwrap_err();
    let _ = unwrap_syntax(err);
}

#[test]
fn number_with_two_dots_rejects() {
    let err = parse("MATCH (n:Item) RETURN 3.14.15").unwrap_err();
    let _ = unwrap_syntax(err);
}

#[test]
fn number_with_trailing_dot_rejects() {
    let err = parse("MATCH (n:Item) RETURN 3.").unwrap_err();
    let _ = unwrap_syntax(err);
}

#[test]
fn number_with_leading_dot_rejects() {
    let err = parse("MATCH (n:Item) RETURN .5").unwrap_err();
    let _ = unwrap_syntax(err);
}

#[test]
fn number_with_exponent_notation_rejects() {
    let err = parse("MATCH (n:Item) RETURN 1e10").unwrap_err();
    let _ = unwrap_syntax(err);
}

#[test]
fn sign_without_digits_rejects() {
    let err = parse("MATCH (n:Item) RETURN -").unwrap_err();
    let _ = unwrap_syntax(err);
}

#[test]
fn unbalanced_open_paren_in_node_pattern_rejects() {
    let err = parse("MATCH (n RETURN n").unwrap_err();
    let _ = unwrap_syntax(err);
}

#[test]
fn unbalanced_open_bracket_in_edge_pattern_rejects() {
    let err = parse("MATCH (n)-[ -> RETURN n").unwrap_err();
    let _ = unwrap_syntax(err);
}

#[test]
fn mismatched_paren_bracket_rejects() {
    let err = parse("MATCH (n] RETURN n").unwrap_err();
    let _ = unwrap_syntax(err);
}

#[test]
fn return_with_empty_parens_rejects() {
    let err = parse("MATCH (n:Item) RETURN ()").unwrap_err();
    let _ = unwrap_syntax(err);
}

#[test]
fn bare_where_without_match_rejects() {
    let err = parse("WHERE n.x = 1").unwrap_err();
    let _ = unwrap_syntax(err);
}

#[test]
fn return_with_no_expression_rejects() {
    let err = parse("MATCH (n:Item) RETURN").unwrap_err();
    let _ = unwrap_syntax(err);
}

#[test]
fn return_with_trailing_comma_rejects() {
    let err = parse("MATCH (n:Item) RETURN n,").unwrap_err();
    let _ = unwrap_syntax(err);
}

#[test]
fn order_by_with_no_expression_rejects() {
    let err = parse("MATCH (n:Item) RETURN n ORDER BY").unwrap_err();
    let _ = unwrap_syntax(err);
}

#[test]
fn limit_with_non_integer_rejects() {
    let err = parse("MATCH (n:Item) RETURN n LIMIT abc").unwrap_err();
    let _ = unwrap_syntax(err);
}

#[test]
fn limit_with_negative_value_rejects() {
    let err = parse("MATCH (n:Item) RETURN n LIMIT -5").unwrap_err();
    let _ = unwrap_syntax(err);
}

#[test]
fn match_keyword_alone_rejects() {
    let err = parse("MATCH RETURN n").unwrap_err();
    let _ = unwrap_syntax(err);
}

#[test]
fn return_with_double_comma_rejects() {
    let err = parse("MATCH (n:Item) RETURN n,,m").unwrap_err();
    let _ = unwrap_syntax(err);
}

#[test]
fn empty_input_rejects() {
    let err = parse("").unwrap_err();
    let (line, col, _, _) = unwrap_syntax(err);
    assert_eq!(line, 1);
    assert_eq!(col, 1);
}

#[test]
fn whitespace_only_input_rejects() {
    let err = parse("   \n  ").unwrap_err();
    let _ = unwrap_syntax(err);
}

#[test]
fn just_return_without_match_rejects() {
    let err = parse("RETURN 1").unwrap_err();
    let _ = unwrap_syntax(err);
}

#[test]
fn label_after_colon_required_rejects() {
    let err = parse("MATCH (n:) RETURN n").unwrap_err();
    let _ = unwrap_syntax(err);
}

#[test]
fn property_value_required_rejects() {
    let err = parse("MATCH (n {x:}) RETURN n").unwrap_err();
    let _ = unwrap_syntax(err);
}

#[test]
fn unterminated_block_comment_rejects() {
    let err = parse("MATCH (n:Item) /* unterm RETURN n").unwrap_err();
    let _ = unwrap_syntax(err);
}

#[test]
fn trailing_garbage_after_return_rejects() {
    let err = parse("MATCH (n:Item) RETURN n GARBAGE").unwrap_err();
    let _ = unwrap_syntax(err);
}

#[test]
fn comparison_missing_rhs_rejects() {
    let err = parse("MATCH (n:Item) WHERE n.x = RETURN n").unwrap_err();
    let _ = unwrap_syntax(err);
}

#[test]
fn bare_property_without_identifier_rejects() {
    let err = parse("MATCH (n:Item) WHERE .x = 1 RETURN n").unwrap_err();
    let _ = unwrap_syntax(err);
}

#[test]
fn empty_where_clause_rejects() {
    let err = parse("MATCH (n:Item) WHERE RETURN n").unwrap_err();
    let _ = unwrap_syntax(err);
}

#[test]
fn dangling_and_rejects() {
    let err = parse("MATCH (n:Item) WHERE n.x = 1 AND RETURN n").unwrap_err();
    let _ = unwrap_syntax(err);
}

#[test]
fn dangling_or_rejects() {
    let err = parse("MATCH (n:Item) WHERE n.x = 1 OR RETURN n").unwrap_err();
    let _ = unwrap_syntax(err);
}

#[test]
fn dangling_not_rejects() {
    let err = parse("MATCH (n:Item) WHERE NOT RETURN n").unwrap_err();
    let _ = unwrap_syntax(err);
}

#[test]
fn identifier_starting_with_digit_rejects() {
    let err = parse("MATCH (1n:Item) RETURN 1n").unwrap_err();
    let _ = unwrap_syntax(err);
}

#[test]
fn reserved_keyword_as_bare_identifier_rejects() {
    let err = parse("MATCH (MATCH:Item) RETURN MATCH").unwrap_err();
    let (_, _, msg, _) = unwrap_syntax(err);
    assert!(msg.contains("MATCH"), "msg: {msg}");
    assert!(
        msg.contains("reserved keyword"),
        "msg should mention 'reserved keyword': {msg}"
    );
}

#[test]
fn reserved_keyword_as_label_rejects() {
    let err = parse("MATCH (n:MATCH) RETURN n").unwrap_err();
    let (_, _, msg, _) = unwrap_syntax(err);
    assert!(msg.contains("MATCH"), "msg: {msg}");
    assert!(
        msg.contains("reserved keyword"),
        "msg should mention 'reserved keyword': {msg}"
    );
}

#[test]
fn keyword_inside_string_literal_does_not_false_positive() {
    let q = parse("MATCH (a:Item) WHERE a.qname = 'CREATE foo' RETURN a").expect("parses");
    assert_eq!(q.match_clauses.len(), 1);
}

#[test]
fn out_of_scope_keyword_inside_double_quoted_string_literal_does_not_false_positive() {
    let q = parse(r#"MATCH (n:Item) WHERE n.name = "DELETE_ME" RETURN n"#).expect("parses");
    assert_eq!(q.match_clauses.len(), 1);
}

#[test]
fn out_of_scope_keyword_as_identifier_substring_does_not_false_positive() {
    let q = parse("MATCH (n:Item) WHERE n.CREATEd_at > 0 RETURN n").expect("parses");
    assert_eq!(q.match_clauses.len(), 1);
}

#[test]
fn merge_inside_string_literal_does_not_false_positive() {
    let q = parse("MATCH (a:Item) WHERE a.qname = 'MERGE foo' RETURN a").expect("parses");
    assert_eq!(q.match_clauses.len(), 1);
}

#[test]
fn set_as_identifier_prefix_does_not_false_positive() {
    let q = parse("MATCH (n:Item) WHERE n.SETtings = 1 RETURN n").expect("parses");
    assert_eq!(q.match_clauses.len(), 1);
}

#[test]
fn newline_inside_double_quoted_string_is_accepted() {
    let q = parse("MATCH (n:Item) WHERE n.x = \"foo\nbar\" RETURN n").expect("parses");
    assert_eq!(q.match_clauses.len(), 1);
}

#[test]
fn parse_match_range_rejects_u32_overflow() {
    let result = parse("MATCH ()-[:CALLS*1..99999999999]->() RETURN count(*) AS n");
    assert!(
        result.is_err(),
        "overflowing variable-length range must return Err, got {result:?}"
    );
}

#[test]
fn parse_limit_rejects_u32_overflow() {
    let result = parse("MATCH (a:Item) RETURN a LIMIT 99999999999");
    assert!(
        result.is_err(),
        "overflowing LIMIT must return Err, got {result:?}"
    );
}

#[test]
fn parse_property_value_rejects_i64_overflow() {
    let result = parse("MATCH (n:Item) WHERE n.x = 99999999999999999999 RETURN n");
    assert!(
        result.is_err(),
        "overflowing int literal must return Err, got {result:?}"
    );
}

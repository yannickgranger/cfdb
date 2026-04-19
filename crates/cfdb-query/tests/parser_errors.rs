//! Parser error messages must carry line/col + actionable suggestion. RFC
//! §14 LLM-specialist finding: callers can only self-correct on "expected X"
//! feedback, not a bare `parse error`.

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
fn delete_rejected() {
    let err = parse("MATCH (a:Item) DELETE a").unwrap_err();
    let (_, _, msg, _) = unwrap_syntax(err);
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
fn garbage_input_reports_location() {
    // Missing MATCH keyword → syntax error pointing somewhere early.
    let err = parse("FOO (a:Item) RETURN a").unwrap_err();
    let (line, col, _msg, _sug) = unwrap_syntax(err);
    assert_eq!(line, 1);
    assert!(col >= 1);
}

#[test]
fn keyword_inside_string_literal_does_not_false_positive() {
    // A string literal that *contains* "CREATE" must not trigger the
    // out-of-scope filter.
    let q = parse("MATCH (a:Item) WHERE a.qname = 'CREATE foo' RETURN a").expect("parses");
    assert_eq!(q.match_clauses.len(), 1);
}

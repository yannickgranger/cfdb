//! Parser error messages must carry line/col + actionable suggestion. RFC
//! §14 LLM-specialist finding: callers can only self-correct on "expected X"
//! feedback, not a bare `parse error`.
//!
//! This file is the **negative corpus** for the cfdb-query parser. The parser
//! is the LLM-callable surface (Cypher-as-DSL for skill bodies and rule
//! files); regressions here surface as confusing user-facing errors in
//! production. The corpus is intentionally dense — every category of
//! malformed input the grammar should reject is covered, with substring
//! assertions on the message/suggestion fields so error-message refactors do
//! not break the corpus.
//!
//! Categories:
//!   A — out-of-scope keywords (CREATE / MERGE / DELETE / SET / REMOVE / CALL / DETACH)
//!   B — multi-statement / semicolon
//!   C — malformed string literals (post-F-008 escape support)
//!   D — malformed number literals
//!   E — unbalanced delimiters in patterns
//!   F — malformed pattern shapes
//!   G — predicate / expression edge cases
//!   H — identifier rules
//!   I — string-literal false-positive scars (keyword inside string must NOT trigger)
//!   J — overflow + audit regression scars (CFDB-QRY-H2 / #272)
//!
//! Property/fuzz testing (`proptest` / `arbitrary`) is a deliberate follow-up
//! — see EPIC #273 Pattern 5 PR body.

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

// ---------------------------------------------------------------------------
// A — Out-of-scope keywords
//
// Every keyword in `OUT_OF_SCOPE` (mod.rs) is rejected with the v0.1 message.
// We assert (a) the message names the offending keyword, (b) the suggestion
// mentions "read-only", and (c) the position points at the keyword itself
// (line 1, col 1 for bare-leading cases).
// ---------------------------------------------------------------------------

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
    // Bare DETACH (no DELETE following) — DETACH alone is in OUT_OF_SCOPE so
    // this trips the keyword scan independently of DELETE.
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
    // `DETACH DELETE` — the OUT_OF_SCOPE list iterates in declared order
    // (CREATE, MERGE, DELETE, SET, REMOVE, CALL, DETACH). `DELETE` matches
    // first, so the error names DELETE at col 8, not DETACH.
    let err = parse("DETACH DELETE n").unwrap_err();
    let (line, col, msg, _) = unwrap_syntax(err);
    assert_eq!(line, 1);
    assert_eq!(col, 8);
    assert!(msg.contains("DELETE"), "msg: {msg}");
}

// ---------------------------------------------------------------------------
// B — Multi-statement / semicolon
// ---------------------------------------------------------------------------

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
    // A single valid query followed by `;` — the semicolon trips the
    // multi-statement check even though there's no second statement after.
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

// ---------------------------------------------------------------------------
// C — Malformed string literals (post-F-008 escape support)
//
// F-008 added `\\`, `\'`, `\"`, `\n`, `\r`, `\t`. The negative cases are
// (a) unterminated strings, (b) mid-escape EOF, (c) unsupported escapes,
// (d) mismatched quote pairs. Note: a literal newline INSIDE a string is
// accepted (multi-line strings are valid), so that case stays positive.
// ---------------------------------------------------------------------------

#[test]
fn unterminated_single_quoted_string_rejects() {
    let err = parse("MATCH (n:Item) WHERE n.x = 'hello RETURN n").unwrap_err();
    let (_, _, _msg, _) = unwrap_syntax(err);
    // Don't pin to a specific message — the chumsky-derived "found end of
    // input" wording is implementation-detail. The signal is: it errored.
}

#[test]
fn unterminated_double_quoted_string_rejects() {
    let err = parse(r#"MATCH (n:Item) WHERE n.x = "hello RETURN n"#).unwrap_err();
    let _ = unwrap_syntax(err);
}

#[test]
fn mid_escape_eof_rejects() {
    // `'a\` — the backslash starts an escape, but EOF arrives before the
    // escaped char. Must not panic.
    let err = parse(r#"MATCH (n:Item) WHERE n.x = 'a\"#).unwrap_err();
    let _ = unwrap_syntax(err);
}

#[test]
fn unsupported_escape_z_rejects_and_lists_supported() {
    // F-008 contract: unsupported escapes carry a message naming the
    // offending sequence and listing the supported set.
    let err = parse(r#"MATCH (n:Item) WHERE n.x = 'a\z' RETURN n"#).unwrap_err();
    let (_, _, msg, _) = unwrap_syntax(err);
    assert!(
        msg.contains("\\z") || msg.contains("unsupported escape"),
        "msg should name \\z or 'unsupported escape': {msg}"
    );
}

#[test]
fn unsupported_escape_double_quoted_rejects() {
    // Same contract for double-quoted strings.
    let err = parse(r#"MATCH (n:Item) WHERE n.x = "a\q" RETURN n"#).unwrap_err();
    let (_, _, msg, _) = unwrap_syntax(err);
    assert!(
        msg.contains("\\q") || msg.contains("unsupported escape"),
        "msg should name \\q or 'unsupported escape': {msg}"
    );
}

#[test]
fn string_spanning_to_eof_rejects() {
    // A backslash-n inside a string is fine (it's the supported `\n`
    // escape). The case here is a legitimately *unclosed* string that runs
    // to end-of-input.
    let err = parse(r#"MATCH (n:Item) WHERE n.x = "foo\nbar"#).unwrap_err();
    let _ = unwrap_syntax(err);
}

#[test]
fn mismatched_quote_kinds_rejects() {
    // Single-quote open, double-quote close — the single-quoted string
    // never sees its terminator, so the whole tail is consumed as string
    // content and the parse fails at EOF.
    let err = parse(r#"MATCH (n:Item) WHERE n.x = 'foo" RETURN n"#).unwrap_err();
    let _ = unwrap_syntax(err);
}

// ---------------------------------------------------------------------------
// D — Malformed number literals
//
// `number_literal_parser` accepts: optional `-` sign, ≥1 digit int part,
// optional `.` + ≥1 digit fractional. It does NOT accept `e`/`E` notation
// (out of scope for v0.1) or leading/trailing dots.
// ---------------------------------------------------------------------------

#[test]
fn number_with_two_dots_rejects() {
    let err = parse("MATCH (n:Item) RETURN 3.14.15").unwrap_err();
    let _ = unwrap_syntax(err);
}

#[test]
fn number_with_trailing_dot_rejects() {
    // `3.` — the int parses, then `.` requires ≥1 digit after, which fails.
    // The trailing dot has no following decimal so the parse aborts.
    let err = parse("MATCH (n:Item) RETURN 3.").unwrap_err();
    let _ = unwrap_syntax(err);
}

#[test]
fn number_with_leading_dot_rejects() {
    // `.5` is not accepted — int part requires ≥1 digit before the dot.
    let err = parse("MATCH (n:Item) RETURN .5").unwrap_err();
    let _ = unwrap_syntax(err);
}

#[test]
fn number_with_exponent_notation_rejects() {
    // v0.1 grammar does NOT support `e`/`E` notation. `1e10` parses `1` as
    // an int, then chokes on `e`.
    let err = parse("MATCH (n:Item) RETURN 1e10").unwrap_err();
    let _ = unwrap_syntax(err);
}

#[test]
fn sign_without_digits_rejects() {
    // Bare `-` with no following digit — the int_part requires ≥1 digit.
    let err = parse("MATCH (n:Item) RETURN -").unwrap_err();
    let _ = unwrap_syntax(err);
}

// ---------------------------------------------------------------------------
// E — Unbalanced delimiters in patterns
// ---------------------------------------------------------------------------

#[test]
fn unbalanced_open_paren_in_node_pattern_rejects() {
    // `MATCH (n RETURN n` — node pattern's `)` never arrives.
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
    // Open with `(`, close with `]`.
    let err = parse("MATCH (n] RETURN n").unwrap_err();
    let _ = unwrap_syntax(err);
}

#[test]
fn return_with_empty_parens_rejects() {
    // `RETURN ()` is not a valid projection — projections take an
    // expression, not a parenthesised empty pattern.
    let err = parse("MATCH (n:Item) RETURN ()").unwrap_err();
    let _ = unwrap_syntax(err);
}

// ---------------------------------------------------------------------------
// F — Malformed pattern shapes
// ---------------------------------------------------------------------------

#[test]
fn bare_where_without_match_rejects() {
    // The grammar requires at least one MATCH/OPTIONAL/UNWIND clause first.
    let err = parse("WHERE n.x = 1").unwrap_err();
    let _ = unwrap_syntax(err);
}

#[test]
fn return_with_no_expression_rejects() {
    // `MATCH (n) RETURN` — projection_list requires ≥1 projection.
    let err = parse("MATCH (n:Item) RETURN").unwrap_err();
    let _ = unwrap_syntax(err);
}

#[test]
fn return_with_trailing_comma_rejects() {
    // `RETURN n,` — projection_list is `separated_by(',')` without
    // allow_trailing, so a trailing comma demands another projection.
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
    // LIMIT requires ≥1 ASCII digit.
    let err = parse("MATCH (n:Item) RETURN n LIMIT abc").unwrap_err();
    let _ = unwrap_syntax(err);
}

#[test]
fn limit_with_negative_value_rejects() {
    // LIMIT u32 — `-5` has a leading `-` that's not an ASCII digit.
    let err = parse("MATCH (n:Item) RETURN n LIMIT -5").unwrap_err();
    let _ = unwrap_syntax(err);
}

#[test]
fn match_keyword_alone_rejects() {
    // `MATCH RETURN n` — MATCH requires ≥1 element.
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
    // No MATCH/OPTIONAL/UNWIND clause first.
    let err = parse("RETURN 1").unwrap_err();
    let _ = unwrap_syntax(err);
}

#[test]
fn label_after_colon_required_rejects() {
    // `MATCH (n:)` — colon without a following identifier.
    let err = parse("MATCH (n:) RETURN n").unwrap_err();
    let _ = unwrap_syntax(err);
}

#[test]
fn property_value_required_rejects() {
    // `{x:}` — prop key without value.
    let err = parse("MATCH (n {x:}) RETURN n").unwrap_err();
    let _ = unwrap_syntax(err);
}

#[test]
fn unterminated_block_comment_rejects() {
    // `/* ...` with no closing `*/` — comments-strip leaves the rest as
    // spaces but the EOF lookback bypasses any meaningful content.
    let err = parse("MATCH (n:Item) /* unterm RETURN n").unwrap_err();
    let _ = unwrap_syntax(err);
}

#[test]
fn trailing_garbage_after_return_rejects() {
    // `MATCH (n) RETURN n GARBAGE` — top-level parser ends with `end()`,
    // so unconsumed input is a parse failure.
    let err = parse("MATCH (n:Item) RETURN n GARBAGE").unwrap_err();
    let _ = unwrap_syntax(err);
}

// ---------------------------------------------------------------------------
// G — Predicate / expression edge cases
// ---------------------------------------------------------------------------

#[test]
fn comparison_missing_rhs_rejects() {
    // `WHERE n.x = RETURN n` — RHS of `=` is required.
    let err = parse("MATCH (n:Item) WHERE n.x = RETURN n").unwrap_err();
    let _ = unwrap_syntax(err);
}

#[test]
fn bare_property_without_identifier_rejects() {
    // `WHERE .x = 1` — property access requires an identifier base.
    let err = parse("MATCH (n:Item) WHERE .x = 1 RETURN n").unwrap_err();
    let _ = unwrap_syntax(err);
}

#[test]
fn empty_where_clause_rejects() {
    // `WHERE` keyword with no predicate.
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
    // `WHERE NOT RETURN n` — NOT requires an atom.
    let err = parse("MATCH (n:Item) WHERE NOT RETURN n").unwrap_err();
    let _ = unwrap_syntax(err);
}

// ---------------------------------------------------------------------------
// H — Identifier rules
// ---------------------------------------------------------------------------

#[test]
fn identifier_starting_with_digit_rejects() {
    // `1n` is not an identifier — `ident_parser` requires alpha or `_` start.
    let err = parse("MATCH (1n:Item) RETURN 1n").unwrap_err();
    let _ = unwrap_syntax(err);
}

#[test]
fn reserved_keyword_as_bare_identifier_rejects() {
    // `MATCH` as an identifier (variable name in node pattern) is rejected
    // by `ident_parser`'s `is_reserved` check, with a specific error
    // message naming the keyword.
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
    // `:MATCH` as a label — labels go through the same ident parser.
    let err = parse("MATCH (n:MATCH) RETURN n").unwrap_err();
    let (_, _, msg, _) = unwrap_syntax(err);
    assert!(msg.contains("MATCH"), "msg: {msg}");
    assert!(
        msg.contains("reserved keyword"),
        "msg should mention 'reserved keyword': {msg}"
    );
}

// ---------------------------------------------------------------------------
// I — String-literal false-positive scars
//
// The byte-scan keyword check (mod.rs::find_keyword) must respect string
// boundaries AND identifier word-boundaries. These tests pin the
// not-a-false-positive cases — companions to the negative cases above
// — so a regression in `classify_string_byte` or `is_ident_byte` surfaces
// here, not in production logs.
// ---------------------------------------------------------------------------

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
    // Companion scar for MERGE — newly added to OUT_OF_SCOPE coverage in
    // this PR, must respect the string boundary too.
    let q = parse("MATCH (a:Item) WHERE a.qname = 'MERGE foo' RETURN a").expect("parses");
    assert_eq!(q.match_clauses.len(), 1);
}

#[test]
fn set_as_identifier_prefix_does_not_false_positive() {
    // `SETtings` — `SET` is a 3-char OUT_OF_SCOPE keyword, but `is_ident_byte`
    // sees `t` after `SET` and the word-boundary check declines.
    let q = parse("MATCH (n:Item) WHERE n.SETtings = 1 RETURN n").expect("parses");
    assert_eq!(q.match_clauses.len(), 1);
}

#[test]
fn newline_inside_double_quoted_string_is_accepted() {
    // The string-literal grammar accepts a literal newline byte inside the
    // string body. This is a positive scar — if a future change forces
    // strings to be single-line, this test will surface it.
    let q = parse("MATCH (n:Item) WHERE n.x = \"foo\nbar\" RETURN n").expect("parses");
    assert_eq!(q.match_clauses.len(), 1);
}

// ---------------------------------------------------------------------------
// J — Overflow + audit regression scars (CFDB-QRY-H2 / #272)
// ---------------------------------------------------------------------------

#[test]
fn parse_match_range_rejects_u32_overflow() {
    // Audit CFDB-QRY-H2 (#272): variable-length range used `unwrapped()` and
    // panicked the process on integer overflow. Must surface as ParseError.
    let result = parse("MATCH ()-[:CALLS*1..99999999999]->() RETURN count(*) AS n");
    assert!(
        result.is_err(),
        "overflowing variable-length range must return Err, got {result:?}"
    );
}

#[test]
fn parse_limit_rejects_u32_overflow() {
    // Audit CFDB-QRY-H2 (#272): LIMIT integer parser used `unwrapped()` and
    // panicked the process on overflow. Must surface as ParseError.
    let result = parse("MATCH (a:Item) RETURN a LIMIT 99999999999");
    assert!(
        result.is_err(),
        "overflowing LIMIT must return Err, got {result:?}"
    );
}

#[test]
fn parse_property_value_rejects_i64_overflow() {
    // Number literal parser must not panic on a value exceeding i64::MAX.
    // The error path should surface an "invalid int literal" via try_map.
    let result = parse("MATCH (n:Item) WHERE n.x = 99999999999999999999 RETURN n");
    assert!(
        result.is_err(),
        "overflowing int literal must return Err, got {result:?}"
    );
}

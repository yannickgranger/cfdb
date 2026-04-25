//! Cypher-subset parser built on chumsky 0.10.
//!
//! Scope: the v0.1 subset locked by study 001 §8.4 — MATCH / OPTIONAL MATCH /
//! WHERE / WITH / UNWIND / RETURN, with property access, regex, IN, NOT EXISTS,
//! aggregation, variable-length paths, and `$param` bindings.
//!
//! Out of scope: CREATE / MERGE / DELETE / SET / REMOVE / CALL / list
//! comprehensions / multi-statement scripts. These are rejected up-front
//! with a clear message.
//!
//! The public entry point is [`parse`]. Errors produced by chumsky are mapped
//! to a crate-local [`ParseError`] so chumsky types do not leak across the
//! crate boundary — this keeps RFC §14's "actionable error messages" concern
//! in one place.

use std::collections::BTreeMap;

use cfdb_core::{OrderBy, Pattern, PropValue, Query, ReturnClause, WithClause};
use chumsky::prelude::*;

mod expression;
mod lexical;
mod match_clause;
mod predicate;
mod projection;

use expression::expr_parser;
use lexical::{bool_or_null_parser, ident_parser, number_literal_parser, string_literal_parser};
use match_clause::{
    edge_pattern_parser, match_clauses_parser, node_pattern_parser, path_pattern_parser,
};
use predicate::predicate_parser;
use projection::{aggregation_parser, projection_parser};

/// Structured parse error. Carries a line/column pair and a best-effort
/// "expected X" suggestion so LLM callers can self-correct.
#[derive(Clone, Debug, thiserror::Error, PartialEq, Eq)]
pub enum ParseError {
    /// Syntactic error at `line:col`. `suggestion` is a terse "expected X"
    /// hint derived from chumsky's Rich error when available.
    #[error("parse error at {line}:{col}: {message}")]
    Syntax {
        line: u32,
        col: u32,
        message: String,
        suggestion: Option<String>,
    },
}

impl ParseError {
    fn syntax(
        source: &str,
        offset: usize,
        message: impl Into<String>,
        suggestion: Option<String>,
    ) -> Self {
        let (line, col) = line_col(source, offset);
        Self::Syntax {
            line,
            col,
            message: message.into(),
            suggestion,
        }
    }
}

fn line_col(source: &str, offset: usize) -> (u32, u32) {
    let clamped = offset.min(source.len());
    let prefix = &source[..clamped];
    let line = prefix.bytes().filter(|b| *b == b'\n').count() as u32 + 1;
    let col = match prefix.rfind('\n') {
        Some(p) => (clamped - p - 1) as u32 + 1,
        None => clamped as u32 + 1,
    };
    (line, col)
}

/// Parse a Cypher-subset source into a `Query` AST.
///
/// Returns `Err(ParseError::Syntax)` on any parse failure, with the first
/// chumsky `Rich` error mapped to a line/column + message + "expected X" hint.
pub fn parse(source: &str) -> Result<Query, ParseError> {
    // Strip `//` line comments and `/* */` block comments into spaces. Both
    // the banned-keyword scan and the chumsky grammar see the scrubbed
    // source, but the original `source` is kept as the anchor for error
    // positions — since comments become spaces, byte positions are
    // preserved, so reported line/col still points at the user's file.
    let scrubbed = strip_comments(source);
    reject_out_of_scope(source, &scrubbed)?;

    let (output, errors) = full_query_parser()
        .parse(scrubbed.as_str())
        .into_output_errors();
    if errors.is_empty() {
        if let Some(q) = output {
            return Ok(q);
        }
    }

    let err = errors
        .into_iter()
        .next()
        .map(|e| {
            let span = e.span();
            let msg = e.reason().to_string();
            let expected: Vec<String> = e
                .expected()
                .map(|x| format!("{x}"))
                .filter(|s| !s.is_empty())
                .collect();
            let suggestion = if expected.is_empty() {
                None
            } else {
                Some(format!("expected one of: {}", expected.join(", ")))
            };
            ParseError::syntax(source, span.start, msg, suggestion)
        })
        .unwrap_or_else(|| ParseError::syntax(source, 0, "unknown parse failure", None));
    Err(err)
}

/// String-literal classification of a single byte for the Cypher-subset
/// scanners. Both [`strip_comments`] and [`find_keyword`] need to skip
/// `'…'` / `"…"` content; keeping the state machine in one place means a
/// future dialect change (raw strings, backtick identifiers, escape
/// sequences) lands in exactly one function (RFC-031 §6 / issue #28).
#[derive(Debug, Clone, Copy)]
enum StringLiteralStep {
    /// Byte is an opening or closing quote — it toggled `in_single` or
    /// `in_double`. Both scanners treat quotes as string-content for
    /// their purposes (the quote itself is pushed verbatim / skipped),
    /// but we distinguish it from a mid-literal byte for clarity.
    Quote,
    /// Byte sits inside a string literal (neither a quote nor outside
    /// any literal). Scanners advance past it without inspecting.
    InsideString,
    /// Byte is outside any string literal. Scanners run their own
    /// comment-detection / keyword-matching logic here.
    OutsideString,
}

/// Advance the string-literal state machine by one byte and classify it.
/// Mutates `in_single` / `in_double` on quote bytes so the caller's next
/// call sees the updated state.
fn classify_string_byte(b: u8, in_single: &mut bool, in_double: &mut bool) -> StringLiteralStep {
    if !*in_double && b == b'\'' {
        *in_single = !*in_single;
        return StringLiteralStep::Quote;
    }
    if !*in_single && b == b'"' {
        *in_double = !*in_double;
        return StringLiteralStep::Quote;
    }
    if *in_single || *in_double {
        StringLiteralStep::InsideString
    } else {
        StringLiteralStep::OutsideString
    }
}

/// Replace `//` line comments and `/* */` block comments with spaces so the
/// rest of the parser never sees them. Positions are preserved byte-for-byte
/// which keeps error line/col correct against the user's original source.
///
/// String literals are respected — `'foo // not a comment'` stays intact —
/// via the shared [`classify_string_byte`] state machine.
fn strip_comments(source: &str) -> String {
    let bytes = source.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    let mut in_single = false;
    let mut in_double = false;
    while i < bytes.len() {
        let b = bytes[i];
        match classify_string_byte(b, &mut in_single, &mut in_double) {
            StringLiteralStep::Quote | StringLiteralStep::InsideString => {
                out.push(b);
                i += 1;
            }
            StringLiteralStep::OutsideString if is_line_comment_start(bytes, i) => {
                consume_line_comment(bytes, &mut i, &mut out);
            }
            StringLiteralStep::OutsideString if is_block_comment_start(bytes, i) => {
                consume_block_comment(bytes, &mut i, &mut out);
            }
            StringLiteralStep::OutsideString => {
                out.push(b);
                i += 1;
            }
        }
    }
    // Safety: strip_comments only replaces bytes with ASCII space / newline
    // and preserves all non-comment bytes verbatim — so multi-byte UTF-8
    // sequences stay valid.
    String::from_utf8(out).expect("comment-stripped source stays valid UTF-8")
}

fn is_line_comment_start(bytes: &[u8], i: usize) -> bool {
    bytes.get(i) == Some(&b'/') && bytes.get(i + 1) == Some(&b'/')
}

fn is_block_comment_start(bytes: &[u8], i: usize) -> bool {
    bytes.get(i) == Some(&b'/') && bytes.get(i + 1) == Some(&b'*')
}

/// Consume a `//` line comment starting at `*i`, pushing a space for every
/// byte except the terminating newline (which is preserved to keep line
/// numbers aligned with the original source). Advances `*i` past the last
/// comment byte — the newline itself (if any) is left for the outer loop
/// to push verbatim.
fn consume_line_comment(bytes: &[u8], i: &mut usize, out: &mut Vec<u8>) {
    while *i < bytes.len() && bytes[*i] != b'\n' {
        out.push(b' ');
        *i += 1;
    }
}

/// Consume a `/* */` block comment starting at `*i`. Pushes two spaces for
/// the opening `/*`, a space-or-newline for each inner byte (newlines are
/// preserved to keep line numbers aligned), and two spaces for the closing
/// `*/`. Advances `*i` past the closing `*/` — or to EOF if the block is
/// unterminated (parser will surface a syntax error further down).
fn consume_block_comment(bytes: &[u8], i: &mut usize, out: &mut Vec<u8>) {
    out.push(b' ');
    out.push(b' ');
    *i += 2;
    while *i + 1 < bytes.len() && !(bytes[*i] == b'*' && bytes[*i + 1] == b'/') {
        out.push(if bytes[*i] == b'\n' { b'\n' } else { b' ' });
        *i += 1;
    }
    if *i + 1 < bytes.len() {
        out.push(b' ');
        out.push(b' ');
        *i += 2;
    }
}

/// Reject v0.1-out-of-scope keywords early with a clear message. We do this
/// before running the grammar because chumsky's "expected token" error on a
/// bare `CREATE` would be confusing ("expected MATCH, found CREATE") vs the
/// true cause ("CREATE is not supported in v0.1").
///
/// `source` is the original (for error-position reporting), `scrubbed` is
/// the comment-stripped form that the scan actually runs against.
fn reject_out_of_scope(source: &str, scrubbed: &str) -> Result<(), ParseError> {
    const OUT_OF_SCOPE: &[&str] = &[
        "CREATE", "MERGE", "DELETE", "SET", "REMOVE", "CALL", "DETACH",
    ];
    for kw in OUT_OF_SCOPE {
        if let Some(pos) = find_keyword(scrubbed, kw) {
            return Err(ParseError::syntax(
                source,
                pos,
                format!("`{kw}` is not supported in the cfdb v0.1 Cypher subset"),
                Some(format!(
                    "remove the {kw} clause — cfdb v0.1 is read-only via the query layer"
                )),
            ));
        }
    }
    if let Some(pos) = scrubbed.find(';') {
        return Err(ParseError::syntax(
            source,
            pos,
            "multi-statement scripts are not supported in the cfdb v0.1 Cypher subset",
            Some("submit one statement at a time".into()),
        ));
    }
    Ok(())
}

fn find_keyword(source: &str, kw: &str) -> Option<usize> {
    let bytes = source.as_bytes();
    let kw_bytes = kw.as_bytes();
    let mut i = 0;
    // Skip content inside string literals to avoid false positives. Shared
    // state machine with `strip_comments` via [`classify_string_byte`].
    let mut in_single = false;
    let mut in_double = false;
    while i < bytes.len() {
        let b = bytes[i];
        match classify_string_byte(b, &mut in_single, &mut in_double) {
            StringLiteralStep::Quote | StringLiteralStep::InsideString => {
                i += 1;
                continue;
            }
            StringLiteralStep::OutsideString => {}
        }
        if i + kw_bytes.len() <= bytes.len() {
            let slice = &bytes[i..i + kw_bytes.len()];
            if slice.eq_ignore_ascii_case(kw_bytes) {
                let before_ok = i == 0 || !is_ident_byte(bytes[i - 1]);
                let after_ok =
                    i + kw_bytes.len() == bytes.len() || !is_ident_byte(bytes[i + kw_bytes.len()]);
                if before_ok && after_ok {
                    return Some(i);
                }
            }
        }
        i += 1;
    }
    None
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

// ---------------------------------------------------------------------------
// Grammar — everything lives in `full_query_parser` so chumsky's deeply-nested
// types get `.boxed()` at each natural stage, bounding the monomorphization
// depth and preventing stack overflow during parser construction.
// ---------------------------------------------------------------------------

type Extra<'a> = extra::Err<Rich<'a, char>>;
type BoxedParser<'a, O> = Boxed<'a, 'a, &'a str, O, Extra<'a>>;

fn full_query_parser<'a>() -> BoxedParser<'a, Query> {
    // ---- Lexical helpers ----
    let ident = ident_parser();
    let string_lit = string_literal_parser();
    let number_lit = number_literal_parser();
    let bool_null = bool_or_null_parser();
    let prop_lit = choice((
        string_lit.clone().map(PropValue::Str),
        bool_null,
        number_lit,
    ))
    .boxed();

    let param_name = just('$').ignore_then(ident.clone()).boxed();

    // ---- Expressions ----
    let expr = expr_parser(ident.clone(), prop_lit.clone(), param_name.clone());

    // ---- Predicates ----
    let predicate = predicate_parser(expr.clone());

    // ---- Node / edge / path patterns ----
    let node_pat = node_pattern_parser(ident.clone(), prop_lit.clone());
    let edge_pat = edge_pattern_parser(ident.clone());
    let path_pat = path_pattern_parser(node_pat.clone(), edge_pat);

    let match_element = choice((path_pat, node_pat.clone().map(Pattern::Node))).boxed();

    // ---- Match clauses (MATCH / OPTIONAL MATCH / UNWIND) ----
    let match_clauses = match_clauses_parser(match_element, ident.clone(), param_name.clone());

    // ---- Projections ----
    let aggregation = aggregation_parser(expr.clone());
    let projection = projection_parser(expr.clone(), aggregation, ident.clone());
    let projection_list = projection
        .clone()
        .separated_by(just(',').padded())
        .at_least(1)
        .collect::<Vec<_>>()
        .boxed();

    // ---- ORDER BY / LIMIT ----
    let order_by = kw("order")
        .ignore_then(kw("by"))
        .ignore_then(
            expr.clone()
                .then(choice((kw("desc").to(true), kw("asc").to(false))).or_not())
                .map(|(e, d)| OrderBy {
                    expr: e,
                    descending: d.unwrap_or(false),
                })
                .separated_by(just(',').padded())
                .at_least(1)
                .collect::<Vec<_>>(),
        )
        .boxed();

    let limit = kw("limit")
        .ignore_then(
            any()
                .filter(|c: &char| c.is_ascii_digit())
                .repeated()
                .at_least(1)
                .to_slice()
                .try_map(|s: &str, span| {
                    s.parse::<u32>()
                        .map_err(|_| Rich::custom(span, "LIMIT integer exceeds u32::MAX"))
                })
                .padded(),
        )
        .boxed();

    // ---- RETURN ----
    let return_clause = kw("return")
        .ignore_then(kw("distinct").or_not().map(|x| x.is_some()))
        .then(projection_list.clone())
        .then(order_by.or_not())
        .then(limit.or_not())
        .map(
            |(((distinct, projections), order_by), limit)| ReturnClause {
                projections,
                order_by: order_by.unwrap_or_default(),
                limit,
                distinct,
            },
        )
        .boxed();

    // ---- WITH ----
    let with_clause = kw("with")
        .ignore_then(projection_list)
        .then(kw("where").ignore_then(predicate.clone()).or_not())
        .map(|(projections, where_clause)| WithClause {
            projections,
            where_clause,
        })
        .boxed();

    // ---- Top-level query ----
    match_clauses
        .then(kw("where").ignore_then(predicate).or_not())
        .then(with_clause.or_not())
        .then(return_clause)
        .then_ignore(end())
        .map(
            |(((match_clauses, where_clause), with_clause), return_clause)| Query {
                match_clauses,
                where_clause,
                with_clause,
                return_clause,
                params: BTreeMap::new(),
            },
        )
        .boxed()
}

// ---------------------------------------------------------------------------
// Shared keyword helper — case-insensitive, padded. Used by every sub-layer.
// ---------------------------------------------------------------------------

fn kw<'a>(word: &'static str) -> BoxedParser<'a, ()> {
    any()
        .filter(move |c: &char| c.is_ascii_alphabetic() || *c == '_')
        .repeated()
        .at_least(1)
        .to_slice()
        .try_map(move |s: &str, span| {
            if s.eq_ignore_ascii_case(word) {
                Ok(())
            } else {
                Err(Rich::custom(span, format!("expected keyword `{word}`")))
            }
        })
        .padded()
        .boxed()
}

#[cfg(test)]
mod tests {
    use super::*;
    use cfdb_core::{Aggregation, ProjectionValue};

    #[test]
    fn parse_minimal_return_count_star() {
        let q = parse("MATCH (a:Item) RETURN count(*) AS n").expect("parses");
        assert_eq!(q.match_clauses.len(), 1);
        match &q.return_clause.projections[0].value {
            ProjectionValue::Aggregation(Aggregation::CountStar) => {}
            other => panic!("expected CountStar, got {other:?}"),
        }
    }

    #[test]
    fn parse_rejects_create() {
        let err = parse("CREATE (a:Item) RETURN a").unwrap_err();
        match err {
            ParseError::Syntax { message, .. } => {
                assert!(message.contains("CREATE"), "message: {message}");
            }
        }
    }

    #[test]
    fn parse_rejects_multi_statement() {
        let err = parse("MATCH (a:Item) RETURN a; MATCH (b:Item) RETURN b").unwrap_err();
        match err {
            ParseError::Syntax { message, .. } => {
                assert!(message.contains("multi-statement"), "message: {message}");
            }
        }
    }
}

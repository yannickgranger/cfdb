//! Lexical layer — identifiers, string/number/bool/null literals.

use cfdb_core::PropValue;
use chumsky::prelude::*;

use super::{kw, BoxedParser};

pub(super) fn ident_parser<'a>() -> BoxedParser<'a, String> {
    let start = any().filter(|c: &char| c.is_ascii_alphabetic() || *c == '_');
    let rest = any()
        .filter(|c: &char| c.is_ascii_alphanumeric() || *c == '_')
        .repeated();
    start
        .then(rest)
        .to_slice()
        .try_map(|s: &str, span| {
            if is_reserved(s) {
                Err(Rich::custom(
                    span,
                    format!("`{s}` is a reserved keyword, not an identifier"),
                ))
            } else {
                Ok(s.to_string())
            }
        })
        .padded()
        .boxed()
}

fn is_reserved(s: &str) -> bool {
    matches!(
        s.to_ascii_uppercase().as_str(),
        "MATCH"
            | "OPTIONAL"
            | "WHERE"
            | "WITH"
            | "RETURN"
            | "UNWIND"
            | "AS"
            | "AND"
            | "OR"
            | "NOT"
            | "IN"
            | "EXISTS"
            | "ORDER"
            | "BY"
            | "LIMIT"
            | "DISTINCT"
            | "DESC"
            | "ASC"
            | "TRUE"
            | "FALSE"
            | "NULL"
    )
}

/// Parses a string literal in single or double quotes with escape support.
///
/// Supported escape sequences (within both `'…'` and `"…"`):
///
/// | Escape | Meaning |
/// |--------|---------|
/// | `\\`   | literal `\`         |
/// | `\'`   | literal `'`         |
/// | `\"`   | literal `"`         |
/// | `\n`   | newline (0x0A)      |
/// | `\r`   | carriage return (0x0D) |
/// | `\t`   | tab (0x09)          |
///
/// Any other escape (e.g. `\z`) is rejected at parse time with a message
/// listing the supported set. Out of scope for v0.1: `\b`, `\f`,
/// `\u{XXXX}`, `\xNN`, raw strings.
///
/// This is the source of truth for "what is a string literal" in cfdb-query;
/// callers that need to recognise quoted text in raw input (e.g. keyword
/// scrubbing in pre-parse passes) MUST honour the same escape semantics.
pub(super) fn string_literal_parser<'a>() -> BoxedParser<'a, String> {
    // `\X` — recognise a backslash, then dispatch on the next char.
    //
    // Unsupported escapes (e.g. `\z`) emit a `Rich::custom` error via
    // `validate` and yield the offending character verbatim so the surrounding
    // `repeated()` continues consuming input rather than terminating mid-string
    // and degrading the error to "expected end of input". Using `validate`
    // instead of `try_map` is load-bearing: `try_map` inside a `repeated()`
    // body causes chumsky to backtrack the consumed `\`, the repeat then
    // stops cleanly, and the original escape error is lost. `validate`
    // commits to the consumed input and surfaces the rich error.
    let escape = just('\\').ignore_then(any().validate(|c: char, e, emitter| match c {
        '\\' => '\\',
        '\'' => '\'',
        '"' => '"',
        'n' => '\n',
        'r' => '\r',
        't' => '\t',
        other => {
            emitter.emit(Rich::custom(
                e.span(),
                format!(
                    "unsupported escape sequence \\{other} (supported: \\\\, \\', \\\", \\n, \\r, \\t)"
                ),
            ));
            other
        }
    }));

    // For each quote flavour, the body is a sequence of (escape | normal char).
    // `none_of` rejects the closing quote and the backslash so the escape arm
    // gets its turn.
    let single_inner = choice((escape, none_of("\\'")))
        .repeated()
        .collect::<String>();
    let single = just('\'').ignore_then(single_inner).then_ignore(just('\''));

    let double_inner = choice((escape, none_of("\\\"")))
        .repeated()
        .collect::<String>();
    let double = just('"').ignore_then(double_inner).then_ignore(just('"'));

    single.or(double).padded().boxed()
}

pub(super) fn number_literal_parser<'a>() -> BoxedParser<'a, PropValue> {
    let sign = just('-').or_not();
    let int_part = any()
        .filter(|c: &char| c.is_ascii_digit())
        .repeated()
        .at_least(1);
    let frac = just('.').then(
        any()
            .filter(|c: &char| c.is_ascii_digit())
            .repeated()
            .at_least(1),
    );
    sign.then(int_part)
        .then(frac.or_not())
        .to_slice()
        .try_map(|s: &str, span| {
            if s.contains('.') {
                s.parse::<f64>()
                    .map(PropValue::Float)
                    .map_err(|_| Rich::custom(span, "invalid float literal"))
            } else {
                s.parse::<i64>()
                    .map(PropValue::Int)
                    .map_err(|_| Rich::custom(span, "invalid int literal"))
            }
        })
        .padded()
        .boxed()
}

pub(super) fn bool_or_null_parser<'a>() -> BoxedParser<'a, PropValue> {
    choice((
        kw("true").to(PropValue::Bool(true)),
        kw("false").to(PropValue::Bool(false)),
        kw("null").to(PropValue::Null),
    ))
    .boxed()
}

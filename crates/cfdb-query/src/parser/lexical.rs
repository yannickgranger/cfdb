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

pub(super) fn string_literal_parser<'a>() -> BoxedParser<'a, String> {
    let single = just('\'')
        .ignore_then(none_of('\'').repeated().to_slice())
        .then_ignore(just('\''));
    let double = just('"')
        .ignore_then(none_of('"').repeated().to_slice())
        .then_ignore(just('"'));
    single
        .or(double)
        .map(|s: &str| s.to_string())
        .padded()
        .boxed()
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

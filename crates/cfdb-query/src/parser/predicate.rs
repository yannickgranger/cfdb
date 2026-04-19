//! Predicate layer — comparisons, regex, IN, NOT EXISTS, AND/OR/NOT, plus the
//! restricted subquery grammar used by `NOT EXISTS { MATCH ... }`.

use std::collections::BTreeMap;

use cfdb_core::{CompareOp, Expr, Pattern, Predicate, PropValue, Query, ReturnClause};
use chumsky::prelude::*;

use super::lexical::{
    bool_or_null_parser, ident_parser, number_literal_parser, string_literal_parser,
};
use super::match_clause::{edge_pattern_parser, node_pattern_parser, path_pattern_parser};
use super::{kw, BoxedParser, Extra};

pub(super) fn predicate_parser<'a>(expr: BoxedParser<'a, Expr>) -> BoxedParser<'a, Predicate> {
    recursive(|pred| {
        let compare_op = choice((
            just("<>").to(CompareOp::Ne),
            just("<=").to(CompareOp::Le),
            just(">=").to(CompareOp::Ge),
            just("=").to(CompareOp::Eq),
            just("<").to(CompareOp::Lt),
            just(">").to(CompareOp::Gt),
        ))
        .padded()
        .boxed();

        let regex = expr
            .clone()
            .then_ignore(just("=~").padded())
            .then(expr.clone())
            .map(|(left, pattern)| Predicate::Regex { left, pattern });

        let in_pred = expr
            .clone()
            .then_ignore(kw("in"))
            .then(expr.clone())
            .map(|(left, right)| Predicate::In { left, right });

        // NOT EXISTS { MATCH ... [WHERE ...] }
        // We allow only a simplified inner query: MATCH ... (no nested
        // subqueries to avoid re-entering the full parser).
        let not_exists = kw("not")
            .ignore_then(kw("exists"))
            .ignore_then(subquery_parser(expr.clone(), pred.clone()))
            .map(|inner| Predicate::NotExists {
                inner: Box::new(inner),
            });

        let cmp = expr
            .clone()
            .then(compare_op)
            .then(expr.clone())
            .map(|((left, op), right)| match op {
                CompareOp::Ne => Predicate::Ne { left, right },
                _ => Predicate::Compare { left, op, right },
            });

        let atom = choice((
            not_exists,
            pred.clone()
                .delimited_by(just('(').padded(), just(')').padded()),
            regex,
            in_pred,
            cmp,
        ))
        .padded()
        .boxed();

        let not = kw("not")
            .ignore_then(atom.clone())
            .map(|p| Predicate::Not(Box::new(p)));

        let unary = choice((not, atom)).boxed();

        let and = unary
            .clone()
            .foldl(kw("and").ignore_then(unary.clone()).repeated(), |l, r| {
                Predicate::And(Box::new(l), Box::new(r))
            })
            .boxed();

        and.clone()
            .foldl(kw("or").ignore_then(and).repeated(), |l, r| {
                Predicate::Or(Box::new(l), Box::new(r))
            })
            .boxed()
    })
    .boxed()
}

// `NOT EXISTS { MATCH (a)-[:CALLS]->(b) [WHERE pred] }` — inner is a pattern
// with an optional WHERE. No nested WITH / RETURN.
fn subquery_parser<'a>(
    expr: BoxedParser<'a, Expr>,
    _pred_hole: impl Parser<'a, &'a str, Predicate, Extra<'a>> + Clone + 'a,
) -> BoxedParser<'a, Query> {
    // We reconstruct the minimum grammar inline to avoid re-entering the full
    // parser path.
    let ident = ident_parser();
    let prop_lit = choice((
        string_literal_parser().map(PropValue::Str),
        bool_or_null_parser(),
        number_literal_parser(),
    ))
    .boxed();

    // NOTE: the inner pattern matching re-uses the same path/node parsers
    // as the top level, but we only need single-hop paths here (the study's
    // F6 case). This is intentional — deeper subqueries can go in v0.2.
    let node_pat = node_pattern_parser(ident.clone(), prop_lit.clone());
    let edge_pat = edge_pattern_parser(ident.clone());
    let path_pat = path_pattern_parser(node_pat.clone(), edge_pat);

    let match_element = choice((path_pat, node_pat.map(Pattern::Node))).boxed();

    // Inner subquery has its own tiny predicate grammar for WHERE — but we
    // actually only need the top-level expression comparisons here. We
    // synthesize a Compare-only predicate parser so we avoid the recursive
    // loop. For v0.1 scope this covers the F6 use cases.
    let inner_cmp_op = choice((
        just("<>").to(CompareOp::Ne),
        just("<=").to(CompareOp::Le),
        just(">=").to(CompareOp::Ge),
        just("=").to(CompareOp::Eq),
        just("<").to(CompareOp::Lt),
        just(">").to(CompareOp::Gt),
    ))
    .padded()
    .boxed();
    let inner_pred = expr
        .clone()
        .then(inner_cmp_op)
        .then(expr.clone())
        .map(|((left, op), right)| match op {
            CompareOp::Ne => Predicate::Ne { left, right },
            _ => Predicate::Compare { left, op, right },
        })
        .boxed();

    kw("match")
        .ignore_then(
            match_element
                .separated_by(just(',').padded())
                .at_least(1)
                .collect::<Vec<_>>(),
        )
        .then(kw("where").ignore_then(inner_pred).or_not())
        .delimited_by(just('{').padded(), just('}').padded())
        .map(|(match_clauses, where_clause)| Query {
            match_clauses,
            where_clause,
            with_clause: None,
            return_clause: ReturnClause {
                projections: vec![],
                order_by: vec![],
                limit: None,
                distinct: false,
            },
            params: BTreeMap::new(),
        })
        .boxed()
}

//! Match layer — node / edge / path patterns and MATCH / OPTIONAL MATCH / UNWIND clauses.

use std::collections::BTreeMap;

use cfdb_core::{
    Direction, EdgeLabel, EdgePattern, Label, NodePattern, PathPattern, Pattern, PropValue,
};
use chumsky::prelude::*;

use super::{kw, BoxedParser};

pub(super) fn node_pattern_parser<'a>(
    ident: BoxedParser<'a, String>,
    prop_lit: BoxedParser<'a, PropValue>,
) -> BoxedParser<'a, NodePattern> {
    let label = just(':')
        .padded()
        .ignore_then(ident.clone())
        .map(Label::new)
        .boxed();

    let prop_entry = ident
        .clone()
        .then_ignore(just(':').padded())
        .then(prop_lit)
        .map(|(k, v)| (k, v))
        .boxed();

    let props_block = prop_entry
        .separated_by(just(',').padded())
        .allow_trailing()
        .collect::<Vec<_>>()
        .delimited_by(just('{').padded(), just('}').padded())
        .map(|entries: Vec<(String, PropValue)>| {
            let mut m = BTreeMap::new();
            for (k, v) in entries {
                m.insert(k, v);
            }
            m
        })
        .boxed();

    ident
        .or_not()
        .then(label.or_not())
        .then(props_block.or_not())
        .map(|((var, label), props)| NodePattern {
            var,
            label,
            props: props.unwrap_or_default(),
        })
        .delimited_by(just('(').padded(), just(')').padded())
        .padded()
        .boxed()
}

pub(super) fn edge_pattern_parser<'a>(
    ident: BoxedParser<'a, String>,
) -> BoxedParser<'a, EdgePattern> {
    let label = just(':')
        .padded()
        .ignore_then(ident.clone())
        .map(EdgeLabel::new)
        .boxed();

    let digits = || {
        any()
            .filter(|c: &char| c.is_ascii_digit())
            .repeated()
            .at_least(1)
            .to_slice()
            .try_map(|s: &str, span| {
                s.parse::<u32>().map_err(|_| {
                    Rich::custom(
                        span,
                        "integer literal exceeds u32::MAX in variable-length range",
                    )
                })
            })
    };

    let range = just('*')
        .ignore_then(digits())
        .then_ignore(just("..").padded())
        .then(digits())
        .boxed();

    ident
        .or_not()
        .then(label.or_not())
        .then(range.or_not())
        .map(|((var, label), var_length)| EdgePattern {
            var,
            label,
            direction: Direction::Undirected, // patched by path_pattern_parser
            var_length,
        })
        .delimited_by(just('[').padded(), just(']').padded())
        .boxed()
}

pub(super) fn path_pattern_parser<'a>(
    node_pat: BoxedParser<'a, NodePattern>,
    edge_pat: BoxedParser<'a, EdgePattern>,
) -> BoxedParser<'a, Pattern> {
    node_pat
        .clone()
        .then(choice((
            just("<-").padded().to(true),
            just("-").padded().to(false),
        )))
        .then(edge_pat)
        .then(choice((
            just("->").padded().to(true),
            just("-").padded().to(false),
        )))
        .then(node_pat)
        .map(|((((from, left_arrow), mut edge), right_arrow), to)| {
            let direction = match (left_arrow, right_arrow) {
                (true, false) => Direction::In,
                (false, true) => Direction::Out,
                _ => Direction::Undirected,
            };
            edge.direction = direction;
            Pattern::Path(PathPattern { from, edge, to })
        })
        .boxed()
}

pub(super) fn match_clauses_parser<'a>(
    match_element: BoxedParser<'a, Pattern>,
    ident: BoxedParser<'a, String>,
    param_name: BoxedParser<'a, String>,
) -> BoxedParser<'a, Vec<Pattern>> {
    let match_clause = kw("match")
        .ignore_then(
            match_element
                .clone()
                .separated_by(just(',').padded())
                .at_least(1)
                .collect::<Vec<_>>(),
        )
        .boxed();

    let optional_clause = kw("optional")
        .ignore_then(kw("match"))
        .ignore_then(
            match_element
                .separated_by(just(',').padded())
                .at_least(1)
                .collect::<Vec<_>>(),
        )
        .map(|ps| {
            ps.into_iter()
                .map(|p| Pattern::Optional(Box::new(p)))
                .collect::<Vec<_>>()
        })
        .boxed();

    let unwind_clause = kw("unwind")
        .ignore_then(param_name)
        .then_ignore(kw("as"))
        .then(ident)
        .map(|(list_param, var)| vec![Pattern::Unwind { list_param, var }])
        .boxed();

    let clause = choice((optional_clause, unwind_clause, match_clause)).boxed();

    clause
        .repeated()
        .at_least(1)
        .collect::<Vec<_>>()
        .map(|vs: Vec<Vec<Pattern>>| vs.into_iter().flatten().collect())
        .boxed()
}

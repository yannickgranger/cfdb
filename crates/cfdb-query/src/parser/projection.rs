//! Projection layer — aggregations (count/collect/size) and RETURN/WITH projections.

use cfdb_core::{Aggregation, Expr, Projection, ProjectionValue};
use chumsky::prelude::*;

use super::{kw, BoxedParser};

pub(super) fn aggregation_parser<'a>(expr: BoxedParser<'a, Expr>) -> BoxedParser<'a, Aggregation> {
    let star = just('*')
        .padded()
        .to(Aggregation::CountStar)
        .delimited_by(just('(').padded(), just(')').padded())
        .boxed();

    let count_star = kw("count").ignore_then(star).boxed();

    let count_distinct = kw("count")
        .ignore_then(
            kw("distinct")
                .ignore_then(expr.clone())
                .delimited_by(just('(').padded(), just(')').padded()),
        )
        .map(Aggregation::CountDistinct)
        .boxed();

    let count_expr = kw("count")
        .ignore_then(
            expr.clone()
                .delimited_by(just('(').padded(), just(')').padded()),
        )
        .map(Aggregation::Count)
        .boxed();

    let collect_distinct = kw("collect")
        .ignore_then(
            kw("distinct")
                .ignore_then(expr.clone())
                .delimited_by(just('(').padded(), just(')').padded()),
        )
        .map(Aggregation::CollectDistinct)
        .boxed();

    let collect_expr = kw("collect")
        .ignore_then(
            expr.clone()
                .delimited_by(just('(').padded(), just(')').padded()),
        )
        .map(Aggregation::Collect)
        .boxed();

    let size_expr = kw("size")
        .ignore_then(expr.delimited_by(just('(').padded(), just(')').padded()))
        .map(Aggregation::Size)
        .boxed();

    choice((
        count_star,
        count_distinct,
        count_expr,
        collect_distinct,
        collect_expr,
        size_expr,
    ))
    .boxed()
}

pub(super) fn projection_parser<'a>(
    expr: BoxedParser<'a, Expr>,
    agg: BoxedParser<'a, Aggregation>,
    ident: BoxedParser<'a, String>,
) -> BoxedParser<'a, Projection> {
    let agg_proj = agg.map(ProjectionValue::Aggregation);
    let raw_proj = expr.map(ProjectionValue::Expr);

    choice((agg_proj, raw_proj))
        .then(kw("as").ignore_then(ident).or_not())
        .map(|(value, alias)| Projection { value, alias })
        .boxed()
}

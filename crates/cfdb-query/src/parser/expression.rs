//! Expression layer — literals, params, lists, function calls, properties, vars.

use cfdb_core::{Expr, PropValue};
use chumsky::prelude::*;

use super::BoxedParser;

pub(super) fn expr_parser<'a>(
    ident: BoxedParser<'a, String>,
    prop_lit: BoxedParser<'a, PropValue>,
    param_name: BoxedParser<'a, String>,
) -> BoxedParser<'a, Expr> {
    recursive(|expr| {
        let literal = prop_lit.clone().map(Expr::Literal);
        let param = param_name.clone().map(Expr::Param);

        let list_lit = expr
            .clone()
            .separated_by(just(',').padded())
            .allow_trailing()
            .collect::<Vec<_>>()
            .delimited_by(just('[').padded(), just(']').padded())
            .map(Expr::List);

        // function call: ident '(' arg (',' arg)* ')'
        let call = ident
            .clone()
            .then(
                expr.clone()
                    .separated_by(just(',').padded())
                    .allow_trailing()
                    .collect::<Vec<_>>()
                    .delimited_by(just('(').padded(), just(')').padded()),
            )
            .map(|(name, args)| Expr::Call { name, args });

        // property access: ident '.' ident
        let property = ident
            .clone()
            .then_ignore(just('.').padded())
            .then(ident.clone())
            .map(|(var, prop)| Expr::Property { var, prop });

        // bare variable: ident (lowest priority)
        let var = ident.clone().map(Expr::Var);

        choice((literal, param, list_lit, call, property, var))
            .padded()
            .boxed()
    })
    .boxed()
}

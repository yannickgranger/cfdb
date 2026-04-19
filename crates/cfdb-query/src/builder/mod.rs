//! Fluent Rust builder API producing a `cfdb_core::Query` AST.
//!
//! The builder is the type-safe authoring surface. It produces the same
//! `Query` value the parser builds, per the architectural invariant from
//! study 001 §8.3 ("two surfaces, one AST").
//!
//! ```
//! use cfdb_query::QueryBuilder;
//! use cfdb_core::Label;
//!
//! let q = QueryBuilder::new()
//!     .match_node("a", Label::new(Label::ITEM))
//!     .return_count_star("n")
//!     .build();
//! assert_eq!(q.match_clauses.len(), 1);
//! ```

use std::collections::BTreeMap;

use cfdb_core::{Param, Pattern, Predicate, Projection, Query, ReturnClause, WithClause};

mod match_clause;
mod params;
mod projection;
mod where_clause;

/// Fluent builder producing a `Query` AST.
#[derive(Default, Debug, Clone)]
pub struct QueryBuilder {
    patterns: Vec<Pattern>,
    where_preds: Vec<Predicate>,
    with_clause: Option<WithClause>,
    projections: Vec<Projection>,
    order_by: Vec<cfdb_core::OrderBy>,
    limit: Option<u32>,
    distinct: bool,
    params: BTreeMap<String, Param>,
}

impl QueryBuilder {
    /// Create an empty builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Finalize and return the `Query`. Panics if `return_items` /
    /// `return_count_star` was never called — a query without a RETURN clause
    /// is a programmer bug, not a recoverable runtime error.
    pub fn build(self) -> Query {
        assert!(
            !self.projections.is_empty(),
            "QueryBuilder::build called without any RETURN projections"
        );
        let return_clause = ReturnClause {
            projections: self.projections,
            order_by: self.order_by,
            limit: self.limit,
            distinct: self.distinct,
        };
        let where_clause = fold_and(self.where_preds);
        Query {
            match_clauses: self.patterns,
            where_clause,
            with_clause: self.with_clause,
            return_clause,
            params: self.params,
        }
    }

    fn push_where(mut self, p: Predicate) -> Self {
        self.where_preds.push(p);
        self
    }
}

fn fold_and(mut preds: Vec<Predicate>) -> Option<Predicate> {
    if preds.is_empty() {
        return None;
    }
    let first = preds.remove(0);
    Some(
        preds
            .into_iter()
            .fold(first, |acc, p| Predicate::And(Box::new(acc), Box::new(p))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use cfdb_core::{Expr, Label, PropValue};

    #[test]
    fn build_bare_match_count_star() {
        let q = QueryBuilder::new()
            .match_node("a", Label::new(Label::ITEM))
            .return_count_star("n")
            .build();
        assert_eq!(q.match_clauses.len(), 1);
        assert!(q.where_clause.is_none());
        assert_eq!(q.return_clause.projections.len(), 1);
    }

    #[test]
    fn where_preds_are_and_folded() {
        let q = QueryBuilder::new()
            .match_node("a", Label::new(Label::ITEM))
            .where_eq(
                Expr::Property {
                    var: "a".into(),
                    prop: "qname".into(),
                },
                Expr::Literal(PropValue::Str("foo".into())),
            )
            .where_ne(
                Expr::Property {
                    var: "a".into(),
                    prop: "crate".into(),
                },
                Expr::Literal(PropValue::Str("bar".into())),
            )
            .return_count_star("n")
            .build();
        match q.where_clause.expect("where clause set") {
            Predicate::And(_, _) => {}
            other => panic!("expected And, got {other:?}"),
        }
    }
}

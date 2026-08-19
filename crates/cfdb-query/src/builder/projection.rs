use cfdb_core::{Aggregation, Expr, Predicate, Projection, ProjectionValue, WithClause};

use super::QueryBuilder;

impl QueryBuilder {
    pub fn with(mut self, projections: Vec<Projection>) -> Self {
        self.with_clause = Some(WithClause {
            projections,
            where_clause: None,
        });
        self
    }

    pub fn with_where(mut self, pred: Predicate) -> Self {
        let w = self
            .with_clause
            .as_mut()
            .expect("QueryBuilder::with_where called before QueryBuilder::with");
        w.where_clause = Some(pred);
        self
    }

    pub fn return_items(mut self, projections: Vec<Projection>) -> Self {
        self.projections = projections;
        self
    }

    pub fn order_by(mut self, expr: Expr, descending: bool) -> Self {
        self.order_by.push(cfdb_core::OrderBy { expr, descending });
        self
    }

    pub fn limit(mut self, n: u32) -> Self {
        self.limit = Some(n);
        self
    }

    pub fn distinct(mut self) -> Self {
        self.distinct = true;
        self
    }

    pub fn return_count_star(mut self, alias: impl Into<String>) -> Self {
        self.projections = vec![Projection {
            value: ProjectionValue::Aggregation(Aggregation::CountStar),
            alias: Some(alias.into()),
        }];
        self
    }
}

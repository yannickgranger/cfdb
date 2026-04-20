//! `RETURN` clause — project (with optional grouping), distinct, order, limit.
//!
//! Ordering defaults to row-sort-key when no `ORDER BY` is given; this is the
//! deterministic output contract the Gate 3 spike relied on.

use std::collections::BTreeSet;

use cfdb_core::fact::PropValue;
use cfdb_core::query::{Expr, ProjectionValue, ReturnClause};
use cfdb_core::result::{Row, RowValue};

use super::util::{expr_alias, projection_alias, row_sort_key, row_value_cmp};
use super::{Binding, Bindings, Evaluator};

impl<'a> Evaluator<'a> {
    pub(super) fn apply_return(&self, table: &[Bindings], ret: &ReturnClause) -> Vec<Row> {
        let has_aggregation = ret
            .projections
            .iter()
            .any(|p| matches!(p.value, ProjectionValue::Aggregation(_)));

        let mut rows: Vec<Row> = if has_aggregation {
            let grouped = self.group_and_aggregate(table, &ret.projections);
            grouped
                .iter()
                .map(|b| bindings_to_row(b, &ret.projections))
                .collect()
        } else {
            table
                .iter()
                .map(|b| self.expr_row_for_bindings(b, &ret.projections))
                .collect()
        };

        if ret.distinct {
            let mut seen: BTreeSet<String> = BTreeSet::new();
            rows.retain(|row| {
                let key = row_sort_key(row);
                seen.insert(key)
            });
        }

        if !ret.order_by.is_empty() {
            let order_keys: Vec<(String, bool)> = ret
                .order_by
                .iter()
                .map(|o| (expr_alias(&o.expr), o.descending))
                .collect();
            rows.sort_by(|a, b| {
                for (key, desc) in &order_keys {
                    let av = a.get(key);
                    let bv = b.get(key);
                    let ord = match (av, bv) {
                        (Some(x), Some(y)) => row_value_cmp(x, y),
                        (Some(_), None) => std::cmp::Ordering::Greater,
                        (None, Some(_)) => std::cmp::Ordering::Less,
                        (None, None) => std::cmp::Ordering::Equal,
                    };
                    let ord = if *desc { ord.reverse() } else { ord };
                    if ord != std::cmp::Ordering::Equal {
                        return ord;
                    }
                }
                std::cmp::Ordering::Equal
            });
        } else {
            rows.sort_by_key(row_sort_key);
        }

        if let Some(limit) = ret.limit {
            rows.truncate(limit as usize);
        }

        rows
    }

    /// Build one row by evaluating each projection expression against the
    /// given bindings. Extracted from the `map(|b| ...)` closure in
    /// [`apply_return`] so the per-projection `v.clone()` does not sit
    /// inside a `for proj in projections` loop (clones-in-loops gate).
    fn expr_row_for_bindings(
        &self,
        b: &Bindings,
        projections: &[cfdb_core::query::Projection],
    ) -> Row {
        projections
            .iter()
            .filter_map(|proj| self.project_expr_for_row(b, proj))
            .collect()
    }

    /// Evaluate a single `Expr`-shaped projection for [`expr_row_for_bindings`].
    /// Returns `None` for aggregation projections (the caller routes those
    /// through [`apply_return`]'s group-and-aggregate branch).
    fn project_expr_for_row(
        &self,
        b: &Bindings,
        proj: &cfdb_core::query::Projection,
    ) -> Option<(String, RowValue)> {
        let alias = projection_alias(proj);
        let ProjectionValue::Expr(e) = &proj.value else {
            return None;
        };
        // Bare `Var` references with `RowValue::List` bindings (produced
        // by a prior WITH `collect()` aggregation) must be surfaced
        // unchanged — `eval_expr` only returns scalars.
        if let Expr::Var(name) = e {
            if let Some(Binding::Value(v @ RowValue::List(_))) = b.get(name) {
                return Some((alias, v.clone()));
            }
        }
        let value = self.eval_expr(e, b).unwrap_or(PropValue::Null);
        Some((alias, RowValue::Scalar(value)))
    }
}

fn bindings_to_row(bindings: &Bindings, projections: &[cfdb_core::query::Projection]) -> Row {
    projections
        .iter()
        .map(|proj| {
            let alias = projection_alias(proj);
            let value = row_value_for_binding(bindings.get(&alias));
            (alias, value)
        })
        .collect()
}

/// Project one [`Binding`] into its serialised [`RowValue`]. Extracted
/// from the projection loop in [`bindings_to_row`] so the scalar clone
/// lands in a helper call rather than inside a `for` body.
fn row_value_for_binding(binding: Option<&Binding>) -> RowValue {
    match binding {
        Some(Binding::Value(v)) => v.clone(),
        _ => RowValue::Scalar(PropValue::Null),
    }
}

//! `WITH` clause — project, group (if any aggregation), re-filter via
//! trailing `WHERE`. Also hosts the shared group-and-aggregate machinery
//! consumed by both `WITH` and `RETURN`.

use std::collections::{BTreeMap, BTreeSet};

use cfdb_core::fact::PropValue;
use cfdb_core::query::{Aggregation, Projection, ProjectionValue, WithClause};
use cfdb_core::result::RowValue;

use super::util::{projection_alias, propvalue_sort_key};
use super::{Binding, Bindings, Evaluator};

impl<'a> Evaluator<'a> {
    pub(super) fn apply_with(&self, table: Vec<Bindings>, with: &WithClause) -> Vec<Bindings> {
        let has_aggregation = with
            .projections
            .iter()
            .any(|p| matches!(p.value, ProjectionValue::Aggregation(_)));

        let grouped = if has_aggregation {
            self.group_and_aggregate(&table, &with.projections)
        } else {
            let mut out = Vec::with_capacity(table.len());
            for bindings in &table {
                let mut row: Bindings = BTreeMap::new();
                for proj in &with.projections {
                    let alias = projection_alias(proj);
                    if let ProjectionValue::Expr(e) = &proj.value {
                        if let Some(v) = self.eval_expr(e, bindings) {
                            row.insert(alias, Binding::Value(RowValue::Scalar(v)));
                        } else {
                            row.insert(alias, Binding::Null);
                        }
                    }
                }
                out.push(row);
            }
            out
        };

        if let Some(pred) = &with.where_clause {
            grouped
                .into_iter()
                .filter(|b| self.eval_predicate(pred, b))
                .collect()
        } else {
            grouped
        }
    }

    pub(super) fn group_and_aggregate(
        &self,
        table: &[Bindings],
        projections: &[Projection],
    ) -> Vec<Bindings> {
        let key_projections: Vec<&Projection> = projections
            .iter()
            .filter(|p| matches!(p.value, ProjectionValue::Expr(_)))
            .collect();
        let agg_projections: Vec<&Projection> = projections
            .iter()
            .filter(|p| matches!(p.value, ProjectionValue::Aggregation(_)))
            .collect();

        // `PropValue` is not `Ord` (it contains `f64`), so groups are keyed by
        // a deterministic string digest of the key vector. `key_values` keeps
        // the raw `PropValue`s alongside for later use in the output row.
        let mut groups: BTreeMap<String, (Vec<PropValue>, Vec<&Bindings>)> = BTreeMap::new();
        let mut order: Vec<String> = Vec::new();

        for bindings in table {
            self.accumulate_group_row(&mut groups, &mut order, bindings, &key_projections);
        }

        let mut out: Vec<Bindings> = Vec::with_capacity(groups.len());
        for key_str in order {
            if let Some(row) =
                self.materialise_group_row(&groups, &key_str, &key_projections, &agg_projections)
            {
                out.push(row);
            }
        }
        out
    }

    /// Per-row accumulator for the group-and-aggregate body of
    /// [`apply_with`]. Compute the key vector, derive its digest, and
    /// route the bindings into the matching group. Extracted from the
    /// `for bindings in table` loop so the `key_str.clone()` and
    /// `key.clone()` live in a helper rather than the outer loop body.
    fn accumulate_group_row<'t>(
        &self,
        groups: &mut BTreeMap<String, (Vec<PropValue>, Vec<&'t Bindings>)>,
        order: &mut Vec<String>,
        bindings: &'t Bindings,
        key_projections: &[&Projection],
    ) {
        let key: Vec<PropValue> = key_projections
            .iter()
            .map(|p| {
                if let ProjectionValue::Expr(e) = &p.value {
                    self.eval_expr(e, bindings).unwrap_or(PropValue::Null)
                } else {
                    PropValue::Null
                }
            })
            .collect();
        let key_str: String = key
            .iter()
            .map(propvalue_sort_key)
            .collect::<Vec<_>>()
            .join("\u{001f}");
        if !groups.contains_key(&key_str) {
            order.push(key_str.clone());
        }
        groups
            .entry(key_str)
            .or_insert_with(|| (key, Vec::new()))
            .1
            .push(bindings);
    }

    /// Per-group emission body for the output loop of [`apply_with`].
    /// Returns the assembled [`Bindings`] or `None` when the group key
    /// has vanished (defensively — shouldn't happen for keys sourced
    /// from the same map).
    fn materialise_group_row(
        &self,
        groups: &BTreeMap<String, (Vec<PropValue>, Vec<&Bindings>)>,
        key_str: &str,
        key_projections: &[&Projection],
        agg_projections: &[&Projection],
    ) -> Option<Bindings> {
        let (key_values, group_rows) = groups.get(key_str)?;
        let mut row: Bindings = BTreeMap::new();
        key_projections
            .iter()
            .zip(key_values.iter())
            .for_each(|(proj, key_val)| {
                let alias = projection_alias(proj);
                row.insert(alias, Binding::Value(RowValue::Scalar(key_val.clone())));
            });
        agg_projections.iter().for_each(|proj| {
            if let ProjectionValue::Aggregation(agg) = &proj.value {
                let alias = projection_alias(proj);
                let value = self.eval_aggregation(agg, group_rows);
                row.insert(alias, Binding::Value(value));
            }
        });
        Some(row)
    }

    fn eval_aggregation(&self, agg: &Aggregation, group: &[&Bindings]) -> RowValue {
        match agg {
            Aggregation::CountStar => RowValue::Scalar(PropValue::Int(group.len() as i64)),
            Aggregation::Count(e) => {
                let n = group
                    .iter()
                    .filter(|b| {
                        self.eval_expr(e, b)
                            .map(|v| !matches!(v, PropValue::Null))
                            .unwrap_or(false)
                    })
                    .count();
                RowValue::Scalar(PropValue::Int(n as i64))
            }
            Aggregation::CountDistinct(e) => {
                let mut seen: BTreeSet<String> = BTreeSet::new();
                for b in group {
                    if let Some(v) = self.eval_expr(e, b) {
                        if !matches!(v, PropValue::Null) {
                            seen.insert(propvalue_sort_key(&v));
                        }
                    }
                }
                RowValue::Scalar(PropValue::Int(seen.len() as i64))
            }
            Aggregation::Collect(e) => {
                let items: Vec<PropValue> =
                    group.iter().filter_map(|b| self.eval_expr(e, b)).collect();
                RowValue::List(items)
            }
            Aggregation::CollectDistinct(e) => {
                let mut seen: BTreeMap<String, PropValue> = BTreeMap::new();
                for b in group {
                    if let Some(v) = self.eval_expr(e, b) {
                        seen.entry(propvalue_sort_key(&v)).or_insert(v);
                    }
                }
                RowValue::List(seen.into_values().collect())
            }
            Aggregation::Size(e) => {
                if let Some(v) = group.first().and_then(|b| self.eval_expr(e, b)) {
                    match v {
                        PropValue::Str(s) => {
                            RowValue::Scalar(PropValue::Int(s.chars().count() as i64))
                        }
                        _ => RowValue::Scalar(PropValue::Null),
                    }
                } else {
                    RowValue::Scalar(PropValue::Null)
                }
            }
        }
    }
}

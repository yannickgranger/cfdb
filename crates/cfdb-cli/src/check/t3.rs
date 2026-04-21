//! T3 trigger runner — same-name-in-≥2-crates raw Pattern A detection.
//!
//! See `super` module doc for the verdict / correlation rationale.
//! Per-row `is_cross_context` + `canonical_candidate` are computed in
//! Rust against the embedded multi-crate cypher and the canonical-crate
//! correlation set.

use std::collections::BTreeSet;
use std::path::Path;

use cfdb_core::fact::PropValue;
use cfdb_core::result::{QueryResult, Row, RowValue};

use crate::commands::parse_and_execute;

use super::t1::{fetch_scalar_set, scalar_str_owned};
use super::{T3Row, T3_CANONICAL_CRATES_CYPHER, T3_CONCEPT_MULTI_CRATE_CYPHER};

/// Run the T3 trigger: execute the embedded multi-crate cypher, fetch
/// the canonical-crate set once, then per-row derive
/// `is_cross_context` (`n_contexts > 1`) and `canonical_candidate`
/// (the first element of `crates[]` that appears in any
/// `:Context.canonical_crate`, or `null` if none). Emit the merged
/// JSON payload with stable ordering.
pub(super) fn run(db: &Path, keyspace: &str) -> Result<usize, crate::CfdbCliError> {
    let raw = parse_and_execute(
        db,
        keyspace,
        T3_CONCEPT_MULTI_CRATE_CYPHER,
        "trigger T3 / Pattern A multi-crate",
    )?;
    let canonical_crates =
        fetch_scalar_set(db, keyspace, T3_CANONICAL_CRATES_CYPHER, "canonical_crate")?;

    let mut rows_out: Vec<T3Row> = Vec::with_capacity(raw.rows.len());
    for row in raw.rows {
        if let Some(t3) = project_t3_row(&row, &canonical_crates) {
            rows_out.push(t3);
        }
    }

    // Stable order — same sort key as the cypher's ORDER BY: n DESC,
    // name ASC. The cypher's projection already sorted by this key,
    // but we re-sort defensively since the Rust-side projection
    // iterates in receive order. Ties on `n` resolve by `name`.
    rows_out.sort_by(|a, b| b.n.cmp(&a.n).then_with(|| a.name.cmp(&b.name)));

    let mut merged = QueryResult::empty();
    for r in rows_out {
        merged.rows.push(r.into_row());
    }

    let row_count = merged.rows.len();
    eprintln!("violations: {row_count} (rule: trigger T3)");

    let as_json = serde_json::to_string_pretty(&merged)?;
    println!("{as_json}");

    Ok(row_count)
}

/// Project one raw cypher row into a [`T3Row`], computing
/// `is_cross_context` + `canonical_candidate` in Rust because the
/// v0.1 cypher subset RETURN clause does not evaluate boolean or
/// outer-bound OPTIONAL MATCH correlations reliably (see the T3 cypher
/// header for the limitations).
fn project_t3_row(row: &Row, canonical_crates: &BTreeSet<String>) -> Option<T3Row> {
    let name = scalar_str_owned(row, "name")?;
    let kind = scalar_str_owned(row, "kind")?;
    let n = scalar_int(row, "n")?;
    let n_crates = scalar_int(row, "n_crates")?;
    let n_contexts = scalar_int(row, "n_contexts")?;
    let crates = list_str_owned(row, "crates");
    let bounded_contexts = list_str_owned(row, "bounded_contexts");
    let qnames = list_str_owned(row, "qnames");
    let files = list_str_owned(row, "files");

    let is_cross_context = n_contexts > 1;
    // Pick the first crate in this row's `crates[]` list that is
    // declared as some `:Context.canonical_crate` — this is the
    // "canonical candidate" in the T3 row shape per issue AC-4.
    // `BTreeSet::contains` is O(log n); `crates[]` is small in
    // practice (workspace-scale, not ecosystem-scale).
    let canonical_candidate = crates
        .iter()
        .find(|c| canonical_crates.contains(*c))
        .cloned();

    Some(T3Row {
        name,
        kind,
        n,
        n_crates,
        n_contexts,
        crates,
        bounded_contexts,
        qnames,
        files,
        is_cross_context,
        canonical_candidate,
    })
}

/// Extract an integer value from a row column. Returns `None` for
/// missing keys, null values, or non-integer values. Matches the
/// `RowValue::Scalar(PropValue::Int(_))` shape produced by
/// `count(...)` aggregations in the v0.1 evaluator.
fn scalar_int(row: &Row, key: &str) -> Option<i64> {
    match row.get(key)? {
        RowValue::Scalar(PropValue::Int(n)) => Some(*n),
        _ => None,
    }
}

/// Extract a list column as owned `Vec<String>`, filtering to
/// scalar-string elements. Matches the `RowValue::List(Vec<PropValue>)`
/// shape produced by `collect(...)` aggregations.
fn list_str_owned(row: &Row, key: &str) -> Vec<String> {
    match row.get(key) {
        Some(RowValue::List(items)) => items
            .iter()
            .filter_map(|p| match p {
                PropValue::Str(s) => Some(s.clone()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

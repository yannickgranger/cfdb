use std::collections::BTreeSet;

use cfdb_core::fact::PropValue;
use cfdb_core::graph::GraphBackend;
use cfdb_core::result::{Row, RowValue};
use cfdb_core::schema::Keyspace;
use cfdb_eval::QueryEngine;

use crate::engine::ClassifyError;

use super::t1::{fetch_scalar_set, scalar_str_owned};
use super::{
    execute, CheckReport, T3Row, TriggerId, T3_CANONICAL_CRATES_CYPHER,
    T3_CONCEPT_MULTI_CRATE_CYPHER,
};

pub(crate) fn run<S: GraphBackend>(
    engine: &QueryEngine<'_, S>,
    ks: &Keyspace,
) -> Result<CheckReport, ClassifyError> {
    let raw = execute(
        engine,
        ks,
        T3_CONCEPT_MULTI_CRATE_CYPHER,
        "trigger T3 / Pattern A multi-crate",
    )?;
    let canonical_crates = fetch_scalar_set(
        engine,
        ks,
        T3_CANONICAL_CRATES_CYPHER,
        "canonical_crate",
        "trigger T3 / canonical_crate probe",
    )?;

    let mut rows_out: Vec<T3Row> = Vec::with_capacity(raw.len());
    for row in raw {
        if let Some(t3) = project_t3_row(&row, &canonical_crates) {
            rows_out.push(t3);
        }
    }

    rows_out.sort_by(|a, b| b.n.cmp(&a.n).then_with(|| a.name.cmp(&b.name)));

    Ok(CheckReport {
        trigger: TriggerId::T3,
        rows: rows_out.into_iter().map(T3Row::into_row).collect(),
        warnings: Vec::new(),
    })
}

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

fn scalar_int(row: &Row, key: &str) -> Option<i64> {
    match row.get(key)? {
        RowValue::Scalar(PropValue::Int(n)) => Some(*n),
        _ => None,
    }
}

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

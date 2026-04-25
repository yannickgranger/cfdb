//! `cfdb diff` — keyspace-to-keyspace delta.
//!
//! Loads both keyspaces via the composition root, emits the sorted-JSONL
//! canonical dump for each (RFC-cfdb.md §12.1), delegates set-algebra to
//! `cfdb_query::diff::compute_diff`, and prints the resulting
//! `DiffEnvelope` as pretty JSON (default) or line-oriented sorted-JSONL
//! (`--format sorted-jsonl`).

use std::path::PathBuf;
use std::str::FromStr;

use cfdb_core::store::StoreBackend;
use cfdb_query::diff::{compute_diff, ChangedFact, DiffEnvelope, DiffFact, KindsFilter};
use serde_json::{json, Value};

use crate::commands::keyspace_path;
use crate::compose;
use crate::output;

/// Output surface for the diff. `json` is the default envelope; the
/// `sorted-jsonl` variant emits each `{added|removed|changed}` fact as its
/// own JSONL line — determinism-friendly for line-by-line CI diffs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DiffFormat {
    Json,
    SortedJsonl,
}

impl FromStr for DiffFormat {
    type Err = crate::CfdbCliError;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        match raw {
            "json" => Ok(Self::Json),
            "sorted-jsonl" => Ok(Self::SortedJsonl),
            other => Err(crate::CfdbCliError::from(format!(
                "diff: --format `{other}` not supported; expected `json` or `sorted-jsonl`"
            ))),
        }
    }
}

pub fn diff(
    db: PathBuf,
    a: String,
    b: String,
    kinds: Option<String>,
    format: String,
) -> Result<(), crate::CfdbCliError> {
    let format = DiffFormat::from_str(&format)?;

    let path_a = keyspace_path(&db, &a);
    let path_b = keyspace_path(&db, &b);
    if !path_a.exists() {
        return Err(format!("keyspace `{a}` not found at {}", path_a.display()).into());
    }
    if !path_b.exists() {
        return Err(format!("keyspace `{b}` not found at {}", path_b.display()).into());
    }

    let kinds_filter = kinds
        .as_deref()
        .map(KindsFilter::from_str)
        .transpose()
        .map_err(|err| crate::CfdbCliError::from(err.to_string()))?;

    let (store_a, ks_a) = compose::load_store(&db, &a)?;
    let (store_b, ks_b) = compose::load_store(&db, &b)?;
    let dump_a = store_a.canonical_dump(&ks_a)?;
    let dump_b = store_b.canonical_dump(&ks_b)?;

    let envelope = compute_diff(&a, &b, &dump_a, &dump_b, kinds_filter.as_ref())
        .map_err(|err| crate::CfdbCliError::from(err.to_string()))?;

    match format {
        DiffFormat::Json => {
            output::emit_json(&envelope)?;
        }
        DiffFormat::SortedJsonl => {
            emit_sorted_jsonl(&envelope)?;
        }
    }
    Ok(())
}

fn emit_sorted_jsonl(envelope: &DiffEnvelope) -> Result<(), crate::CfdbCliError> {
    // Header line preserves the envelope's scalar metadata so downstream
    // parsers can correlate each fact line with (a, b, schema_version).
    let header = json!({
        "op": "header",
        "a": envelope.a,
        "b": envelope.b,
        "schema_version": envelope.schema_version,
    });
    println!("{}", serde_json::to_string(&header)?);

    for fact in &envelope.added {
        println!("{}", fact_line("added", fact)?);
    }
    for fact in &envelope.removed {
        println!("{}", fact_line("removed", fact)?);
    }
    for fact in &envelope.changed {
        println!("{}", changed_line(fact)?);
    }
    for warning in &envelope.warnings {
        let row = json!({ "op": "warning", "message": warning });
        println!("{}", serde_json::to_string(&row)?);
    }
    Ok(())
}

fn fact_line(op: &str, fact: &DiffFact) -> Result<String, serde_json::Error> {
    let row: Value = json!({
        "op": op,
        "kind": fact.kind,
        "envelope": fact.envelope,
    });
    serde_json::to_string(&row)
}

fn changed_line(fact: &ChangedFact) -> Result<String, serde_json::Error> {
    let row: Value = json!({
        "op": "changed",
        "kind": fact.kind,
        "a": fact.a,
        "b": fact.b,
    });
    serde_json::to_string(&row)
}

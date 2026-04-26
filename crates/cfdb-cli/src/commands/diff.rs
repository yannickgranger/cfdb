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

use crate::compose;
use crate::output;
use crate::output::OutputFormat;

pub fn diff(
    db: PathBuf,
    a: String,
    b: String,
    kinds: Option<String>,
    format: String,
) -> Result<(), crate::CfdbCliError> {
    // `cfdb diff` accepts `json` (default envelope) and `sorted-jsonl` (one
    // line per `{added|removed|changed}` fact). The shared `OutputFormat`
    // enum carries other variants (text, table) that this verb does not
    // accept; the allowlist below enforces the per-handler subset.
    let format = OutputFormat::from_str(&format)?
        .require_one_of(&[OutputFormat::Json, OutputFormat::SortedJsonl], "diff")?;

    compose::ensure_keyspace_exists(&db, &a)?;
    compose::ensure_keyspace_exists(&db, &b)?;

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
        OutputFormat::Json => {
            output::emit_json(&envelope)?;
        }
        OutputFormat::SortedJsonl => {
            emit_sorted_jsonl(&envelope)?;
        }
        // Other variants are filtered out by `require_one_of` above; the
        // type system can't see that, so we name the contract here.
        _ => unreachable!("diff allowlist is restricted to Json | SortedJsonl"),
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

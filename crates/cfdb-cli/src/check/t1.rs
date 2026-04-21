//! T1 trigger runner — concept-declared-in-TOML-but-missing-in-code.
//!
//! See `super` module doc for the verdict / correlation rationale.
//! The three sub-verdicts (CONCEPT_UNWIRED, MISSING_CANONICAL_CRATE,
//! STALE_RFC_REFERENCE) are computed in Rust against four primitive
//! cypher reads, then projected into the merged `QueryResult` payload.

use std::collections::BTreeSet;
use std::path::Path;

use cfdb_core::fact::PropValue;
use cfdb_core::result::{QueryResult, Row, RowValue, Warning, WarningKind};

use crate::commands::parse_and_execute;

use super::{
    ContextRow, Finding, T1_CONTEXT_INVENTORY_CYPHER, T1_CRATE_NAMES_CYPHER,
    T1_ITEM_BOUNDED_CONTEXTS_CYPHER, T1_RFC_DOCS_CYPHER,
};

/// Run the T1 trigger: fetch the four correlation sets, compute the
/// three anti-join sub-verdicts in Rust, emit the merged payload.
pub(super) fn run(db: &Path, keyspace: &str) -> Result<usize, crate::CfdbCliError> {
    let contexts = fetch_contexts(db, keyspace)?;
    let crate_names = fetch_scalar_set(db, keyspace, T1_CRATE_NAMES_CYPHER, "name")?;
    let item_contexts = fetch_scalar_set(db, keyspace, T1_ITEM_BOUNDED_CONTEXTS_CYPHER, "bc")?;
    let rfc_haystack = fetch_rfc_haystack(db, keyspace)?;

    let mut findings: Vec<Finding> = Vec::new();
    for ctx in &contexts {
        if !item_contexts.contains(&ctx.name) {
            findings.push(Finding {
                verdict: "CONCEPT_UNWIRED",
                context_name: ctx.name.clone(),
                canonical_crate: ctx.canonical_crate.clone(),
                owning_rfc: ctx.owning_rfc.clone(),
                evidence: ctx.name.clone(),
            });
        }
        if let Some(canonical) = ctx.canonical_crate.as_deref() {
            if !canonical.is_empty() && !crate_names.contains(canonical) {
                findings.push(Finding {
                    verdict: "MISSING_CANONICAL_CRATE",
                    context_name: ctx.name.clone(),
                    canonical_crate: ctx.canonical_crate.clone(),
                    owning_rfc: ctx.owning_rfc.clone(),
                    evidence: canonical.to_string(),
                });
            }
        }
        if let Some(rfc) = ctx.owning_rfc.as_deref() {
            if !rfc.is_empty() && !rfc_haystack.iter().any(|hay| hay.contains(rfc)) {
                findings.push(Finding {
                    verdict: "STALE_RFC_REFERENCE",
                    context_name: ctx.name.clone(),
                    canonical_crate: ctx.canonical_crate.clone(),
                    owning_rfc: ctx.owning_rfc.clone(),
                    evidence: rfc.to_string(),
                });
            }
        }
    }

    // Determinism: stable order regardless of the per-context
    // verdict-check order. `(context_name, verdict)` is the canonical
    // sort key — same shape as the cypher file's `ORDER BY`.
    findings.sort_by(|a, b| {
        a.context_name
            .cmp(&b.context_name)
            .then_with(|| a.verdict.cmp(b.verdict))
    });

    let mut merged = QueryResult::empty();
    for f in findings {
        merged.rows.push(f.into_row());
    }

    if rfc_haystack.is_empty() {
        merged.warnings.push(Warning {
            kind: WarningKind::EmptyResult,
            message: "no :RfcDoc nodes in keyspace — STALE_RFC_REFERENCE sub-verdict is \
                      evaluated against an empty RFC document set. Any `owning_rfc` tag will \
                      surface as stale. Run `cfdb enrich-rfc-docs --db <db> --keyspace <ks> \
                      --workspace <path>` to populate the doc inventory before checking T1."
                .to_string(),
            suggestion: Some(
                "cfdb enrich-rfc-docs --db <db> --keyspace <ks> --workspace <path>".to_string(),
            ),
        });
    }

    let row_count = merged.rows.len();
    eprintln!("violations: {row_count} (rule: trigger T1)");

    let as_json = serde_json::to_string_pretty(&merged)?;
    println!("{as_json}");

    Ok(row_count)
}

/// Execute the embedded `:Context` inventory cypher and project each
/// row into a `ContextRow`. Non-string props in the returned rows are
/// treated as null (defensive — the extractor only emits string
/// values for these keys, but the cypher layer is untyped).
fn fetch_contexts(db: &Path, keyspace: &str) -> Result<Vec<ContextRow>, crate::CfdbCliError> {
    let result = parse_and_execute(
        db,
        keyspace,
        T1_CONTEXT_INVENTORY_CYPHER,
        "trigger T1 / :Context inventory",
    )?;
    let contexts = result
        .rows
        .into_iter()
        .filter_map(|row| {
            let name = scalar_str_owned(&row, "context_name")?;
            Some(ContextRow {
                name,
                canonical_crate: scalar_str_owned(&row, "canonical_crate"),
                owning_rfc: scalar_str_owned(&row, "owning_rfc"),
            })
        })
        .collect();
    Ok(contexts)
}

/// Execute a simple `MATCH … RETURN col` cypher and collect the
/// requested column's scalar-string values into a deduplicating set.
/// Missing rows / non-string values are skipped.
pub(super) fn fetch_scalar_set(
    db: &Path,
    keyspace: &str,
    cypher: &str,
    col: &str,
) -> Result<BTreeSet<String>, crate::CfdbCliError> {
    let rule_tag = format!("trigger T1 / {col} probe");
    let result = parse_and_execute(db, keyspace, cypher, &rule_tag)?;
    Ok(result
        .rows
        .into_iter()
        .filter_map(|row| scalar_str_owned(&row, col))
        .collect())
}

/// Pull every `:RfcDoc.path` and `:RfcDoc.title` into a single vector
/// of strings. STALE_RFC_REFERENCE tests whether any element of the
/// vector contains the `owning_rfc` tag as a substring — same
/// semantics the cypher's `r.path =~ tag OR r.title =~ tag` would have
/// if the evaluator supported outer-bound regex in OPTIONAL MATCH
/// (it does not, per the cypher file header).
fn fetch_rfc_haystack(db: &Path, keyspace: &str) -> Result<Vec<String>, crate::CfdbCliError> {
    let result = parse_and_execute(
        db,
        keyspace,
        T1_RFC_DOCS_CYPHER,
        "trigger T1 / :RfcDoc probe",
    )?;
    let mut out = Vec::with_capacity(result.rows.len() * 2);
    for row in &result.rows {
        if let Some(path) = scalar_str_owned(row, "path") {
            out.push(path);
        }
        if let Some(title) = scalar_str_owned(row, "title") {
            out.push(title);
        }
    }
    Ok(out)
}

/// Extract a `RowValue::Scalar(PropValue::Str)` into an owned `String`.
/// Returns `None` for missing keys, null values, or non-string values.
pub(super) fn scalar_str_owned(row: &Row, key: &str) -> Option<String> {
    match row.get(key)? {
        RowValue::Scalar(PropValue::Str(s)) => Some(s.clone()),
        _ => None,
    }
}

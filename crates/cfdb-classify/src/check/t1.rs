//! T1 trigger runner — concept-declared-in-TOML-but-missing-in-code.
//!
//! See `super` module doc for the verdict / correlation rationale.
//! The three sub-verdicts (CONCEPT_UNWIRED, MISSING_CANONICAL_CRATE,
//! STALE_RFC_REFERENCE) are computed in Rust against four primitive
//! cypher reads, then projected into the [`CheckReport`].

use std::collections::BTreeSet;

use cfdb_core::fact::PropValue;
use cfdb_core::graph::GraphBackend;
use cfdb_core::result::{Row, RowValue, Warning, WarningKind};
use cfdb_core::schema::Keyspace;
use cfdb_eval::QueryEngine;

use crate::engine::ClassifyError;

use super::{
    execute, CheckReport, ContextRow, T1Row, TriggerId, T1_CONTEXT_INVENTORY_CYPHER,
    T1_CRATE_NAMES_CYPHER, T1_ITEM_BOUNDED_CONTEXTS_CYPHER, T1_RFC_DOCS_CYPHER,
};

/// Run the T1 trigger: fetch the four correlation sets, compute the
/// three anti-join sub-verdicts in Rust, return the report.
pub(crate) fn run<S: GraphBackend>(
    engine: &QueryEngine<'_, S>,
    ks: &Keyspace,
) -> Result<CheckReport, ClassifyError> {
    let contexts = fetch_contexts(engine, ks)?;
    let crate_names = fetch_scalar_set(
        engine,
        ks,
        T1_CRATE_NAMES_CYPHER,
        "name",
        "trigger T1 / name probe",
    )?;
    let item_contexts = fetch_scalar_set(
        engine,
        ks,
        T1_ITEM_BOUNDED_CONTEXTS_CYPHER,
        "bc",
        "trigger T1 / bc probe",
    )?;
    let rfc_haystack = fetch_rfc_haystack(engine, ks)?;

    let mut findings: Vec<T1Row> = Vec::new();
    for ctx in &contexts {
        collect_findings_for_context(
            ctx,
            &item_contexts,
            &crate_names,
            &rfc_haystack,
            &mut findings,
        );
    }

    // Determinism: stable order regardless of the per-context
    // verdict-check order. `(context_name, verdict)` is the canonical
    // sort key — same shape as the cypher file's `ORDER BY`.
    findings.sort_by(|a, b| {
        a.context_name
            .cmp(&b.context_name)
            .then_with(|| a.verdict.cmp(b.verdict))
    });

    let rows: Vec<Row> = findings.into_iter().map(T1Row::into_row).collect();

    let mut warnings = Vec::new();
    if rfc_haystack.is_empty() {
        warnings.push(Warning {
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

    Ok(CheckReport {
        trigger: TriggerId::T1,
        rows,
        warnings,
    })
}

/// Per-context check pipeline: probe the three sub-verdicts and push
/// any matching findings into the accumulator. Extracted from `run`
/// to keep clones out of the outer iteration body and to keep `run`'s
/// cognitive complexity below the workspace ceiling.
fn collect_findings_for_context(
    ctx: &ContextRow,
    item_contexts: &BTreeSet<String>,
    crate_names: &BTreeSet<String>,
    rfc_haystack: &[String],
    out: &mut Vec<T1Row>,
) {
    if let Some(f) = check_concept_unwired(ctx, item_contexts) {
        out.push(f);
    }
    if let Some(f) = check_missing_canonical_crate(ctx, crate_names) {
        out.push(f);
    }
    if let Some(f) = check_stale_rfc_reference(ctx, rfc_haystack) {
        out.push(f);
    }
}

/// CONCEPT_UNWIRED: a `:Context` row exists in the TOML but no `:Item`
/// carries the matching `bounded_context` prop.
fn check_concept_unwired(ctx: &ContextRow, item_contexts: &BTreeSet<String>) -> Option<T1Row> {
    if item_contexts.contains(&ctx.name) {
        return None;
    }
    Some(finding_for(ctx, "CONCEPT_UNWIRED", ctx.name.clone()))
}

/// MISSING_CANONICAL_CRATE: the `:Context.canonical_crate` value names
/// a crate the workspace does not actually contain.
fn check_missing_canonical_crate(
    ctx: &ContextRow,
    crate_names: &BTreeSet<String>,
) -> Option<T1Row> {
    let canonical = ctx.canonical_crate.as_deref()?;
    if canonical.is_empty() || crate_names.contains(canonical) {
        return None;
    }
    Some(finding_for(
        ctx,
        "MISSING_CANONICAL_CRATE",
        canonical.to_string(),
    ))
}

/// STALE_RFC_REFERENCE: the `:Context.owning_rfc` tag does not appear
/// as a substring in any `:RfcDoc.path` or `:RfcDoc.title`.
fn check_stale_rfc_reference(ctx: &ContextRow, rfc_haystack: &[String]) -> Option<T1Row> {
    let rfc = ctx.owning_rfc.as_deref()?;
    if rfc.is_empty() || rfc_haystack.iter().any(|hay| hay.contains(rfc)) {
        return None;
    }
    Some(finding_for(ctx, "STALE_RFC_REFERENCE", rfc.to_string()))
}

/// Construct a `T1Row` from `(ctx, verdict, evidence)`. Centralises
/// the per-finding field copy so the per-context loop body in `run`
/// holds no `.clone()` calls.
fn finding_for(ctx: &ContextRow, verdict: &'static str, evidence: String) -> T1Row {
    T1Row {
        verdict,
        context_name: ctx.name.clone(),
        canonical_crate: ctx.canonical_crate.clone(),
        owning_rfc: ctx.owning_rfc.clone(),
        evidence,
    }
}

/// Execute the embedded `:Context` inventory cypher and project each
/// row into a `ContextRow`. Non-string props in the returned rows are
/// treated as null (defensive — the extractor only emits string
/// values for these keys, but the cypher layer is untyped).
fn fetch_contexts<S: GraphBackend>(
    engine: &QueryEngine<'_, S>,
    ks: &Keyspace,
) -> Result<Vec<ContextRow>, ClassifyError> {
    let rows = execute(
        engine,
        ks,
        T1_CONTEXT_INVENTORY_CYPHER,
        "trigger T1 / :Context inventory",
    )?;
    let contexts = rows
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
/// Missing rows / non-string values are skipped. `rule` names the
/// embedded text in a parse error.
pub(super) fn fetch_scalar_set<S: GraphBackend>(
    engine: &QueryEngine<'_, S>,
    ks: &Keyspace,
    cypher: &str,
    col: &str,
    rule: &'static str,
) -> Result<BTreeSet<String>, ClassifyError> {
    let rows = execute(engine, ks, cypher, rule)?;
    Ok(rows
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
fn fetch_rfc_haystack<S: GraphBackend>(
    engine: &QueryEngine<'_, S>,
    ks: &Keyspace,
) -> Result<Vec<String>, ClassifyError> {
    let rows = execute(engine, ks, T1_RFC_DOCS_CYPHER, "trigger T1 / :RfcDoc probe")?;
    let mut out = Vec::with_capacity(rows.len() * 2);
    for row in &rows {
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

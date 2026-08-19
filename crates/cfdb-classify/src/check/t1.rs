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

fn check_concept_unwired(ctx: &ContextRow, item_contexts: &BTreeSet<String>) -> Option<T1Row> {
    if item_contexts.contains(&ctx.name) {
        return None;
    }
    Some(finding_for(ctx, "CONCEPT_UNWIRED", ctx.name.clone()))
}

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

fn check_stale_rfc_reference(ctx: &ContextRow, rfc_haystack: &[String]) -> Option<T1Row> {
    let rfc = ctx.owning_rfc.as_deref()?;
    if rfc.is_empty() || rfc_haystack.iter().any(|hay| hay.contains(rfc)) {
        return None;
    }
    Some(finding_for(ctx, "STALE_RFC_REFERENCE", rfc.to_string()))
}

fn finding_for(ctx: &ContextRow, verdict: &'static str, evidence: String) -> T1Row {
    T1Row {
        verdict,
        context_name: ctx.name.clone(),
        canonical_crate: ctx.canonical_crate.clone(),
        owning_rfc: ctx.owning_rfc.clone(),
        evidence,
    }
}

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

pub(super) fn scalar_str_owned(row: &Row, key: &str) -> Option<String> {
    match row.get(key)? {
        RowValue::Scalar(PropValue::Str(s)) => Some(s.clone()),
        _ => None,
    }
}

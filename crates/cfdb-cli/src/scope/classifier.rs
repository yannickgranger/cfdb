use cfdb_core::store::StoreBackend;
use cfdb_core::{Param, PropValue};
use cfdb_query::{
    list_items_matching as compose_list_items_matching, parse, CanonicalCandidate, DebtClass,
    Finding,
};

use super::helpers::{
    canonical_candidate_from_row, crates_for_context, finding_from_row, scalar_str,
};
use super::{
    CLASSIFIER_CANONICAL_BYPASS_CYPHER, CLASSIFIER_CONTEXT_HOMONYM_CYPHER,
    CLASSIFIER_DUPLICATED_FEATURE_CYPHER, CLASSIFIER_RANDOM_SCATTERING_CYPHER,
    CLASSIFIER_UNFINISHED_REFACTOR_CYPHER, CLASSIFIER_UNWIRED_CYPHER, HSB_BY_NAME_CYPHER,
};

/// Static list of (class, cypher source) pairs. Iteration order matches
/// [`DebtClass::variants`] so the orchestrator run order is deterministic
/// — load-bearing for G1.
pub(super) fn classifier_rules() -> [(DebtClass, &'static str); 6] {
    [
        (
            DebtClass::DuplicatedFeature,
            CLASSIFIER_DUPLICATED_FEATURE_CYPHER,
        ),
        (DebtClass::ContextHomonym, CLASSIFIER_CONTEXT_HOMONYM_CYPHER),
        (
            DebtClass::UnfinishedRefactor,
            CLASSIFIER_UNFINISHED_REFACTOR_CYPHER,
        ),
        (
            DebtClass::RandomScattering,
            CLASSIFIER_RANDOM_SCATTERING_CYPHER,
        ),
        (
            DebtClass::CanonicalBypass,
            CLASSIFIER_CANONICAL_BYPASS_CYPHER,
        ),
        (DebtClass::Unwired, CLASSIFIER_UNWIRED_CYPHER),
    ]
}

/// Parse, bind `$context`, and execute one classifier rule. Returns
/// the `Finding` rows projected from the result. Missing inputs
/// (absent HIR enrichment, absent concept TOML, etc.) surface as
/// empty result rows — this is the correct degradation, and the
/// warning pass reports the dependency gap.
pub(super) fn run_classifier_rule(
    store: &cfdb_petgraph::PetgraphStore,
    ks: &cfdb_core::schema::Keyspace,
    context: &str,
    cypher: &str,
) -> Result<Vec<Finding>, crate::CfdbCliError> {
    let mut parsed =
        parse(cypher).map_err(|e| format!("parse error in embedded classifier rule: {e}"))?;
    parsed.params.insert(
        "context".to_string(),
        Param::Scalar(PropValue::Str(context.to_string())),
    );
    // Per-rule execution is infallible beyond store-level errors;
    // missing props silently return empty rows (parser / evaluator
    // absent-prop semantics), which lets the orchestrator tolerate
    // keyspaces extracted without HIR / concepts / reachability.
    let result = match store.execute(ks, &parsed) {
        Ok(r) => r,
        // A store-level execution error on a classifier rule is a
        // keyspace shape mismatch (e.g. `:EntryPoint` label absent
        // because the keyspace was extracted with the syn-only
        // extractor). Treat as "classifier rule cannot run against
        // this keyspace" — return empty rows, let the warning path
        // document the degradation.
        Err(_) => return Ok(Vec::new()),
    };
    Ok(result.rows.iter().filter_map(finding_from_row).collect())
}

/// Pull the context-filtered inventory rows + derive the per-crate LOC
/// approximation. Factored out of [`build_scope_inventory`] to keep each
/// helper under the cognitive-complexity ceiling.
pub(super) fn query_findings_in_context(
    store: &cfdb_petgraph::PetgraphStore,
    ks: &cfdb_core::schema::Keyspace,
    context: &str,
) -> Result<(Vec<Finding>, std::collections::BTreeMap<String, u64>), crate::CfdbCliError> {
    let inventory_query = compose_list_items_matching(".*", None, false);
    let inventory_result = store.execute(ks, &inventory_query)?;
    let mut findings_in_context: Vec<Finding> = Vec::with_capacity(inventory_result.rows.len());
    let mut loc_per_crate: std::collections::BTreeMap<String, u64> =
        std::collections::BTreeMap::new();
    for row in &inventory_result.rows {
        let row_context = scalar_str(row, "bounded_context").unwrap_or("");
        if row_context != context {
            continue;
        }
        if let Some(finding) = finding_from_row(row) {
            *loc_per_crate.entry(finding.crate_name.clone()).or_insert(0) += 1;
            findings_in_context.push(finding);
        }
    }
    findings_in_context.sort();
    Ok((findings_in_context, loc_per_crate))
}

/// Run the embedded `hsb-by-name` rule and project each matching row into
/// a canonical candidate if at least one crate belongs to the context.
pub(super) fn query_canonical_candidates(
    store: &cfdb_petgraph::PetgraphStore,
    ks: &cfdb_core::schema::Keyspace,
    context: &str,
) -> Result<Vec<CanonicalCandidate>, crate::CfdbCliError> {
    let hsb_parsed = parse(HSB_BY_NAME_CYPHER)
        .map_err(|e| format!("parse error in embedded hsb-by-name template: {e}"))?;
    let hsb_result = store.execute(ks, &hsb_parsed)?;
    let crates_in_context = crates_for_context(store, ks, context)?;
    Ok(hsb_result
        .rows
        .iter()
        .filter_map(|row| canonical_candidate_from_row(row, &crates_in_context))
        .collect())
}

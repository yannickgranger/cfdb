use cfdb_core::store::StoreBackend;
use cfdb_core::{Param, PropValue};
use cfdb_query::{
    list_items_matching as compose_list_items_matching, parse, CanonicalCandidate, DebtClass,
    Finding,
};

use super::helpers::{canonical_candidate_from_row, crates_for_context, finding_from_row};
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

/// Build an inventory query pre-filtered to a single bounded context by
/// embedding `WHERE item.bounded_context = $context` in the Cypher AST.
/// Pushing the predicate into the evaluator avoids materialising all rows
/// before the Rust filter — the root cause of the 13 GB OOM in issue #167.
pub(super) fn compose_inventory_query_for_context(context: &str) -> cfdb_core::query::Query {
    use cfdb_core::query::{CompareOp, Expr, Predicate};
    let mut q = compose_list_items_matching(".*", None, false);
    let context_pred = Predicate::Compare {
        left: Expr::Property {
            var: "item".into(),
            prop: "bounded_context".into(),
        },
        op: CompareOp::Eq,
        right: Expr::Param("context".into()),
    };
    q.where_clause = Some(match q.where_clause.take() {
        Some(existing) => Predicate::And(Box::new(existing), Box::new(context_pred)),
        None => context_pred,
    });
    q.params.insert(
        "context".into(),
        Param::Scalar(PropValue::Str(context.into())),
    );
    q
}

/// Pull the context-filtered inventory rows + derive the per-crate LOC
/// approximation. Factored out of [`build_scope_inventory`] to keep each
/// helper under the cognitive-complexity ceiling.
pub(super) fn query_findings_in_context(
    store: &cfdb_petgraph::PetgraphStore,
    ks: &cfdb_core::schema::Keyspace,
    context: &str,
) -> Result<(Vec<Finding>, std::collections::BTreeMap<String, u64>), crate::CfdbCliError> {
    let inventory_query = compose_inventory_query_for_context(context);
    let inventory_result = store.execute(ks, &inventory_query)?;
    let mut findings_in_context: Vec<Finding> = Vec::with_capacity(inventory_result.rows.len());
    let mut loc_per_crate: std::collections::BTreeMap<String, u64> =
        std::collections::BTreeMap::new();
    for row in &inventory_result.rows {
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

#[cfg(test)]
mod tests_memory_169 {
    //! Regression test for issue #169 (`memory(scope): push $context filter
    //! into Cypher`).
    //!
    //! Prior behaviour: `query_findings_in_context` called
    //! `compose_list_items_matching(".*", None, false)` — an unbounded
    //! query returning every `:Item` across all contexts — then filtered in
    //! Rust with `if row_context != context { continue; }`. On a 148k-item
    //! keyspace the evaluator materialised every row before the Rust
    //! filter could throw them away, a direct contributor to the 13 GB OOM
    //! documented in parent issue #167.
    //!
    //! The fix pushes the `bounded_context = $context` constraint into the
    //! Cypher query. This test asserts the structural invariant without
    //! executing the query: the composed `Query` AST must carry a predicate
    //! (or parameter binding) that constrains `item.bounded_context`.
    //!
    //! The test imports `super::compose_inventory_query_for_context` — a
    //! `pub(super)` composer the fix introduces. Any equivalent refactor
    //! that exposes the composed inventory `Query` for inspection is
    //! acceptable; rename the symbol and update this `use` if so.
    use cfdb_core::query::{CompareOp, Expr, Predicate, Query};

    use super::compose_inventory_query_for_context;

    #[test]
    fn context_filter_is_pushed_into_cypher_not_rust() {
        let q = compose_inventory_query_for_context("ctx_a");
        assert!(
            query_constrains_bounded_context(&q),
            "expected query to constrain `item.bounded_context` at the \
             Cypher layer (regression for #169). query={q:?}"
        );
    }

    fn query_constrains_bounded_context(q: &Query) -> bool {
        q.where_clause
            .as_ref()
            .is_some_and(predicate_constrains_bounded_context)
    }

    fn predicate_constrains_bounded_context(p: &Predicate) -> bool {
        let touches_bc = |e: &Expr| {
            matches!(e, Expr::Property { prop, .. } if prop == "bounded_context")
        };
        match p {
            Predicate::Compare {
                left,
                op: CompareOp::Eq,
                right,
            } => touches_bc(left) || touches_bc(right),
            Predicate::In { left, .. } => touches_bc(left),
            Predicate::And(a, b) | Predicate::Or(a, b) => {
                predicate_constrains_bounded_context(a)
                    || predicate_constrains_bounded_context(b)
            }
            Predicate::Not(inner) => predicate_constrains_bounded_context(inner),
            _ => false,
        }
    }
}

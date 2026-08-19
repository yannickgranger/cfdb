use cfdb_core::graph::GraphBackend;
use cfdb_core::schema::Keyspace;
use cfdb_core::{ParamBinding, PropValue};
use cfdb_eval::QueryEngine;
use cfdb_query::{list_items_matching as compose_list_items_matching, parse};

use super::helpers::{canonical_candidate_from_row, crates_for_context, finding_from_row};
use crate::engine::ClassifyError;
use crate::explain::ExplainSink;
use crate::rules::{
    CLASSIFIER_CANONICAL_BYPASS_CYPHER, CLASSIFIER_CONTEXT_HOMONYM_CYPHER,
    CLASSIFIER_DUPLICATED_FEATURE_CYPHER, CLASSIFIER_RANDOM_SCATTERING_CYPHER,
    CLASSIFIER_UNFINISHED_REFACTOR_CYPHER, CLASSIFIER_UNWIRED_CYPHER,
    CLASSIFIER_UNWIRED_PRODUCTION_CYPHER, HSB_BY_NAME_CYPHER,
};
use crate::taxonomy::{CanonicalCandidate, DebtClass, Finding};

pub(super) fn classifier_rules(production_only: bool) -> [(DebtClass, &'static str); 6] {
    let unwired_cypher = if production_only {
        CLASSIFIER_UNWIRED_PRODUCTION_CYPHER
    } else {
        CLASSIFIER_UNWIRED_CYPHER
    };
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
        (DebtClass::Unwired, unwired_cypher),
    ]
}

pub(super) fn run_classifier_rule<S: GraphBackend>(
    engine: &QueryEngine<'_, S>,
    ks: &Keyspace,
    context: &str,
    cypher: &str,
    sink: &ExplainSink,
) -> Result<Vec<Finding>, ClassifyError> {
    let mut parsed = parse(cypher).map_err(|source| ClassifyError::Parse {
        rule: "classifier rule",
        source,
    })?;
    parsed.params.insert(
        "context".to_string(),
        ParamBinding::Scalar(PropValue::Str(context.to_string())),
    );
    let result = match sink.run(engine, ks, &parsed) {
        Ok(r) => r,
        Err(_) => return Ok(Vec::new()),
    };
    Ok(result.rows.iter().filter_map(finding_from_row).collect())
}

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
        ParamBinding::Scalar(PropValue::Str(context.into())),
    );
    q
}

pub(super) fn query_findings_in_context<S: GraphBackend>(
    engine: &QueryEngine<'_, S>,
    ks: &Keyspace,
    context: &str,
    sink: &ExplainSink,
) -> Result<(Vec<Finding>, std::collections::BTreeMap<String, u64>), ClassifyError> {
    let inventory_query = compose_inventory_query_for_context(context);
    let inventory_result = sink.run(engine, ks, &inventory_query)?;
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

pub(super) fn query_canonical_candidates<S: GraphBackend>(
    engine: &QueryEngine<'_, S>,
    ks: &Keyspace,
    context: &str,
    sink: &ExplainSink,
) -> Result<Vec<CanonicalCandidate>, ClassifyError> {
    let hsb_parsed = parse(HSB_BY_NAME_CYPHER).map_err(|source| ClassifyError::Parse {
        rule: "hsb-by-name template",
        source,
    })?;
    let hsb_result = sink.run(engine, ks, &hsb_parsed)?;
    let crates_in_context = crates_for_context(engine, ks, context)?;
    Ok(hsb_result
        .rows
        .iter()
        .filter_map(|row| canonical_candidate_from_row(row, &crates_in_context))
        .collect())
}

#[cfg(test)]
mod tests_memory_169 {
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
        let touches_bc =
            |e: &Expr| matches!(e, Expr::Property { prop, .. } if prop == "bounded_context");
        match p {
            Predicate::Compare {
                left,
                op: CompareOp::Eq,
                right,
            } => touches_bc(left) || touches_bc(right),
            Predicate::In { left, .. } => touches_bc(left),
            Predicate::And(a, b) | Predicate::Or(a, b) => {
                predicate_constrains_bounded_context(a) || predicate_constrains_bounded_context(b)
            }
            Predicate::Not(inner) => predicate_constrains_bounded_context(inner),
            _ => false,
        }
    }
}

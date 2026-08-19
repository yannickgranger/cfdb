use std::collections::BTreeSet;

use cfdb_core::graph::GraphBackend;
use cfdb_core::result::{Warning, WarningKind};
use cfdb_core::schema::Keyspace;
use cfdb_eval::QueryEngine;

use crate::engine::ClassifyError;
use crate::explain::ExplainSink;
use crate::taxonomy::{DebtClass, ScopeInventory};

mod classifier;
mod helpers;

use classifier::{query_canonical_candidates, query_findings_in_context, run_classifier_rule};
pub(crate) use helpers::validate_context;

pub(crate) fn build_scope_inventory<S: GraphBackend>(
    engine: &QueryEngine<'_, S>,
    ks: &Keyspace,
    context: &str,
    sink: &ExplainSink,
    production_only: bool,
) -> Result<ScopeInventory, ClassifyError> {
    let (findings_in_context, loc_per_crate) =
        query_findings_in_context(engine, ks, context, sink)?;

    let mut inventory = ScopeInventory::new(context, ks.as_str());
    inventory.loc_per_crate = loc_per_crate;
    let _ = findings_in_context;

    inventory.canonical_candidates = query_canonical_candidates(engine, ks, context, sink)?;
    inventory.canonical_candidates.sort();

    populate_findings_by_class(engine, ks, context, &mut inventory, sink, production_only)?;

    attach_scope_warnings(&mut inventory);
    Ok(inventory)
}

pub(crate) fn populate_findings_by_class<S: GraphBackend>(
    engine: &QueryEngine<'_, S>,
    ks: &Keyspace,
    context: &str,
    inventory: &mut ScopeInventory,
    sink: &ExplainSink,
    production_only: bool,
) -> Result<(), ClassifyError> {
    for (class, cypher) in classifier::classifier_rules(production_only) {
        let findings = run_classifier_rule(engine, ks, context, cypher, sink)?;
        if let Some(bucket) = inventory.findings_by_class.get_mut(&class) {
            bucket.extend(findings);
            bucket.sort();
            bucket.dedup();
        }
    }
    Ok(())
}

pub(crate) fn populate_findings_by_class_restricted<S: GraphBackend>(
    engine: &QueryEngine<'_, S>,
    ks: &Keyspace,
    context: &str,
    restrict_to: &BTreeSet<String>,
    inventory: &mut ScopeInventory,
    sink: &ExplainSink,
) -> Result<(), ClassifyError> {
    populate_findings_by_class(engine, ks, context, inventory, sink, false)?;
    retain_findings_by_qname(inventory, restrict_to);
    Ok(())
}

pub(crate) fn retain_findings_by_qname(
    inventory: &mut ScopeInventory,
    restrict_to: &BTreeSet<String>,
) {
    for bucket in inventory.findings_by_class.values_mut() {
        bucket.retain(|finding| restrict_to.contains(&finding.qname));
    }
}

pub(crate) fn attach_scope_warnings(inventory: &mut ScopeInventory) {
    DebtClass::variants()
        .iter()
        .filter(|class| {
            inventory
                .findings_by_class
                .get(class)
                .map(|v| v.is_empty())
                .unwrap_or(true)
        })
        .filter_map(|class| class_empty_bucket_note(*class))
        .for_each(|message| {
            inventory.warnings.push(Warning {
                kind: WarningKind::EmptyResult,
                message,
                suggestion: None,
            });
        });
    inventory.warnings.push(Warning {
        kind: WarningKind::EmptyResult,
        message: "`reachability_map` is `null` in v0.1 — CALLS / :CallSite edges \
                  require cfdb-hir-extractor (addendum §A1.2); ships in v0.2."
            .to_string(),
        suggestion: None,
    });
    if !inventory.loc_per_crate.is_empty() {
        inventory.warnings.push(Warning {
            kind: WarningKind::EmptyResult,
            message: "`loc_per_crate` reports :Item count per crate, not true \
                      lines-of-code (LOC requires cfdb-hir-extractor — v0.2)."
                .to_string(),
            suggestion: None,
        });
    }
}

pub(crate) fn class_empty_bucket_note(class: DebtClass) -> Option<String> {
    let reason = match class {
        DebtClass::DuplicatedFeature => {
            "findings_by_class.duplicated_feature is empty — no same-context \
             struct/enum/trait homonyms in this context (inputs: :Item.name, \
             :Item.bounded_context — always present in a syn-only extract)"
        }
        DebtClass::ContextHomonym => {
            "findings_by_class.context_homonym is empty — no cross-context \
             signature-divergent fn/method pairs in this context (inputs: \
             :Item.signature, :Item.bounded_context — always present in a \
             syn-only extract, same as duplicated_feature/unfinished_refactor)"
        }
        DebtClass::UnfinishedRefactor => {
            "findings_by_class.unfinished_refactor is empty — no \
             #[deprecated] items in this context (inputs: :Item.is_deprecated, \
             :Item.bounded_context — always present in a syn-only extract)"
        }
        DebtClass::RandomScattering => {
            "findings_by_class.random_scattering is empty — no Pattern B \
             fork findings in this context. If the keyspace was extracted \
             without --features hir, :EntryPoint nodes and CALLS edges are \
             absent and this class degrades to no findings; run `cfdb \
             extract --features hir` to enable."
        }
        DebtClass::CanonicalBypass => {
            "findings_by_class.canonical_bypass is empty — no CANONICAL_FOR \
             unreachable items in this context. Requires both `cfdb \
             enrich-concepts` (CANONICAL_FOR edges from .cfdb/concepts/*.toml) \
             AND `cfdb enrich-reachability` (reachable_from_entry attr, \
             HIR-dependent). Concept-specific BYPASS_REACHABLE / BYPASS_DEAD \
             rules remain available for per-concept triage."
        }
        DebtClass::Unwired => {
            "findings_by_class.unwired is empty — no unreachable fn/method \
             items in this context. Requires `cfdb enrich-reachability` \
             (HIR-dependent). On a keyspace without HIR, every fn is \
             trivially unreachable in the graph's view; the classifier \
             therefore returns empty rather than flooding with false \
             positives."
        }
    };
    Some(reason.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::taxonomy::Finding;

    fn finding(qname: &str) -> Finding {
        Finding {
            qname: qname.to_string(),
            name: qname.rsplit("::").next().unwrap_or(qname).to_string(),
            kind: "struct".to_string(),
            crate_name: "test".to_string(),
            file: "test.rs".to_string(),
            line: 1,
            bounded_context: "test".to_string(),
        }
    }

    fn inventory_with_findings(entries: &[(DebtClass, &[&str])]) -> ScopeInventory {
        let mut inv = ScopeInventory::new("ctx", "sha");
        for (class, qnames) in entries {
            if let Some(bucket) = inv.findings_by_class.get_mut(class) {
                for q in *qnames {
                    bucket.push(finding(q));
                }
            }
        }
        inv
    }

    #[test]
    fn retain_findings_by_qname_empty_restrict_clears_all_buckets() {
        let mut inv = inventory_with_findings(&[
            (DebtClass::DuplicatedFeature, &["a::X", "b::Y"]),
            (DebtClass::ContextHomonym, &["c::Z"]),
        ]);
        retain_findings_by_qname(&mut inv, &BTreeSet::new());
        for (class, bucket) in &inv.findings_by_class {
            assert!(
                bucket.is_empty(),
                "class {class:?} should be empty after empty-restrict filter"
            );
        }
    }

    #[test]
    fn retain_findings_by_qname_keeps_only_matching_qnames() {
        let mut inv = inventory_with_findings(&[
            (DebtClass::DuplicatedFeature, &["keep::X", "drop::Y"]),
            (DebtClass::UnfinishedRefactor, &["keep::Z", "drop::W"]),
        ]);
        let restrict: BTreeSet<String> = ["keep::X", "keep::Z"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        retain_findings_by_qname(&mut inv, &restrict);
        let dup = &inv.findings_by_class[&DebtClass::DuplicatedFeature];
        assert_eq!(dup.len(), 1);
        assert_eq!(dup[0].qname, "keep::X");
        let unfin = &inv.findings_by_class[&DebtClass::UnfinishedRefactor];
        assert_eq!(unfin.len(), 1);
        assert_eq!(unfin[0].qname, "keep::Z");
    }

    #[test]
    fn retain_findings_by_qname_unrelated_qnames_yield_empty_buckets() {
        let mut inv = inventory_with_findings(&[(DebtClass::CanonicalBypass, &["present::X"])]);
        let restrict: BTreeSet<String> = ["absent::Z".into()].into_iter().collect();
        retain_findings_by_qname(&mut inv, &restrict);
        assert!(inv.findings_by_class[&DebtClass::CanonicalBypass].is_empty());
    }
}

//! Enumeration of the 7 enrichment passes per RFC-039 §3.1.
//!
//! Each `PassDef` carries everything the runner needs to dispatch one
//! pass: the canonical name (matches `cfdb enrich-<name>`), the path of
//! the Cypher template (added by Issues #343–#349), the optional ratio
//! threshold (None for hard-equality passes), and the feature gate.

use crate::thresholds;

/// Feature flag the pass requires. Default = no flag (PR-time CI).
/// Non-default variants run only in the nightly job per RFC §3.3.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeatureGate {
    /// PR-time eligible — no feature flag required.
    Default,
    /// `--features hir` — requires `cfdb-hir-extractor` + `:EntryPoint`
    /// nodes. Nightly only.
    Hir,
    /// `--features quality-metrics` — requires syn re-parse for
    /// cyclomatic + unwrap_count. Nightly only.
    QualityMetrics,
    /// `--features git-enrich` — requires libgit2 against the workspace
    /// `.git`. Nightly only.
    GitEnrich,
}

/// Static descriptor for one of the 7 passes.
#[derive(Debug, Clone, Copy)]
pub struct PassDef {
    /// Pass name as accepted by `cfdb enrich-<name>` and `cfdb violations`
    /// rule lookup. Stable identifier; do not rename without bumping every
    /// `.cfdb/queries/self-enrich-*.cypher` cross-reference.
    pub name: &'static str,
    /// Path of the Cypher template file relative to the workspace root.
    /// Templates contain `{{ threshold }}` placeholders for ratio passes.
    /// Materialized to a tempfile by [`crate::runner`] before subprocess
    /// invocation. Files added by Issues #343–#349.
    pub query_template_path: &'static str,
    /// Ratio threshold (percentage 0–100) for ratio-based passes.
    /// `None` for hard-equality passes (deprecation, rfc-docs, concepts).
    pub threshold: Option<u32>,
    /// Cargo feature flag the parent `cfdb` binary must be built with.
    pub feature_required: FeatureGate,
}

impl PassDef {
    /// Static catalogue of all 7 passes. Order matches RFC §3.1 table.
    pub const fn all() -> &'static [PassDef] {
        &[
            PassDef {
                name: "enrich-deprecation",
                query_template_path: ".cfdb/queries/self-enrich-deprecation.cypher",
                threshold: None,
                feature_required: FeatureGate::Default,
            },
            PassDef {
                name: "enrich-rfc-docs",
                query_template_path: ".cfdb/queries/self-enrich-rfc-docs.cypher",
                threshold: None,
                feature_required: FeatureGate::Default,
            },
            PassDef {
                name: "enrich-bounded-context",
                query_template_path: ".cfdb/queries/self-enrich-bounded-context.cypher",
                threshold: Some(thresholds::MIN_BC_COVERAGE_PCT),
                feature_required: FeatureGate::Default,
            },
            PassDef {
                name: "enrich-concepts",
                query_template_path: ".cfdb/queries/self-enrich-concepts.cypher",
                threshold: None,
                feature_required: FeatureGate::Default,
            },
            PassDef {
                name: "enrich-reachability",
                query_template_path: ".cfdb/queries/self-enrich-reachability.cypher",
                threshold: Some(thresholds::MIN_REACHABILITY_PCT),
                feature_required: FeatureGate::Hir,
            },
            PassDef {
                name: "enrich-metrics",
                query_template_path: ".cfdb/queries/self-enrich-metrics.cypher",
                threshold: Some(thresholds::MIN_METRICS_COVERAGE_PCT),
                feature_required: FeatureGate::QualityMetrics,
            },
            PassDef {
                name: "enrich-git-history",
                query_template_path: ".cfdb/queries/self-enrich-git-history.cypher",
                threshold: Some(thresholds::MIN_GIT_COVERAGE_PCT),
                feature_required: FeatureGate::GitEnrich,
            },
        ]
    }

    /// Look up a pass by name. Returns `None` for unknown names —
    /// `main.rs` exits 1 with a "valid passes:" enumeration in that case.
    pub fn by_name(name: &str) -> Option<&'static PassDef> {
        Self::all().iter().find(|p| p.name == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// All 7 RFC-039 passes are present.
    #[test]
    fn all_seven_passes_enumerated() {
        let names: Vec<&str> = PassDef::all().iter().map(|p| p.name).collect();
        assert_eq!(
            names,
            vec![
                "enrich-deprecation",
                "enrich-rfc-docs",
                "enrich-bounded-context",
                "enrich-concepts",
                "enrich-reachability",
                "enrich-metrics",
                "enrich-git-history",
            ]
        );
    }

    /// Default-feature passes (PR-time eligible) match RFC §3.3.
    #[test]
    fn default_feature_passes_are_pr_time_set() {
        let default: Vec<&str> = PassDef::all()
            .iter()
            .filter(|p| p.feature_required == FeatureGate::Default)
            .map(|p| p.name)
            .collect();
        assert_eq!(
            default,
            vec![
                "enrich-deprecation",
                "enrich-rfc-docs",
                "enrich-bounded-context",
                "enrich-concepts",
            ]
        );
    }

    /// Feature-gated passes route to nightly per RFC §3.3.
    #[test]
    fn nightly_passes_have_correct_feature_gates() {
        assert_eq!(
            PassDef::by_name("enrich-reachability").map(|p| p.feature_required),
            Some(FeatureGate::Hir)
        );
        assert_eq!(
            PassDef::by_name("enrich-metrics").map(|p| p.feature_required),
            Some(FeatureGate::QualityMetrics)
        );
        assert_eq!(
            PassDef::by_name("enrich-git-history").map(|p| p.feature_required),
            Some(FeatureGate::GitEnrich)
        );
    }

    /// Ratio passes carry thresholds; hard-equality passes do not.
    #[test]
    fn threshold_assignment_matches_rfc_table() {
        let with_threshold: Vec<&str> = PassDef::all()
            .iter()
            .filter(|p| p.threshold.is_some())
            .map(|p| p.name)
            .collect();
        // The 4 ratio-based invariants per RFC §3.1.
        assert_eq!(
            with_threshold,
            vec![
                "enrich-bounded-context",
                "enrich-reachability",
                "enrich-metrics",
                "enrich-git-history",
            ]
        );
    }

    /// `by_name` round-trip + None on unknown.
    #[test]
    fn by_name_lookup() {
        assert_eq!(
            PassDef::by_name("enrich-concepts").map(|p| p.name),
            Some("enrich-concepts")
        );
        assert!(PassDef::by_name("enrich-bogus").is_none());
        assert!(PassDef::by_name("").is_none());
    }

    /// Query template paths follow the `self-enrich-*` convention from
    /// RFC §2 deliverable 1 (renamed from `dogfood-enrich-*` per
    /// ddd-specialist Q4).
    #[test]
    fn query_template_paths_use_self_enrich_prefix() {
        for p in PassDef::all() {
            assert!(
                p.query_template_path
                    .starts_with(".cfdb/queries/self-enrich-"),
                "{} template path {} does not match self-enrich-* convention",
                p.name,
                p.query_template_path
            );
            assert!(
                p.query_template_path.ends_with(".cypher"),
                "{} template path must end with .cypher",
                p.name
            );
        }
    }
}

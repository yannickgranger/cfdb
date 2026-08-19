use crate::thresholds;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeatureGate {
    Default,
    Hir,
    QualityMetrics,
    GitEnrich,
}

#[derive(Debug, Clone, Copy)]
pub struct PassDef {
    pub name: &'static str,
    pub query_template_path: &'static str,
    pub threshold: Option<u32>,
    pub feature_required: FeatureGate,
    pub cli_takes_workspace: bool,
}

impl PassDef {
    pub const fn all() -> &'static [PassDef] {
        &[
            PassDef {
                name: "enrich-deprecation",
                query_template_path: ".cfdb/queries/self-enrich-deprecation.cypher",
                threshold: thresholds::DEPRECATION_THRESHOLD,
                feature_required: FeatureGate::Default,
                cli_takes_workspace: false,
            },
            PassDef {
                name: "enrich-rfc-docs",
                query_template_path: ".cfdb/queries/self-enrich-rfc-docs.cypher",
                threshold: thresholds::RFC_DOCS_THRESHOLD,
                feature_required: FeatureGate::Default,
                cli_takes_workspace: true,
            },
            PassDef {
                name: "enrich-bounded-context",
                query_template_path: ".cfdb/queries/self-enrich-bounded-context.cypher",
                threshold: thresholds::BC_COVERAGE_THRESHOLD,
                feature_required: FeatureGate::Default,
                cli_takes_workspace: true,
            },
            PassDef {
                name: "enrich-concepts",
                query_template_path: ".cfdb/queries/self-enrich-concepts.cypher",
                threshold: thresholds::CONCEPTS_THRESHOLD,
                feature_required: FeatureGate::Default,
                cli_takes_workspace: true,
            },
            PassDef {
                name: "enrich-reachability",
                query_template_path: ".cfdb/queries/self-enrich-reachability.cypher",
                threshold: thresholds::REACHABILITY_THRESHOLD,
                feature_required: FeatureGate::Hir,
                cli_takes_workspace: false,
            },
            PassDef {
                name: "enrich-metrics",
                query_template_path: ".cfdb/queries/self-enrich-metrics.cypher",
                threshold: thresholds::METRICS_COVERAGE_THRESHOLD,
                feature_required: FeatureGate::QualityMetrics,
                cli_takes_workspace: true,
            },
            PassDef {
                name: "enrich-git-history",
                query_template_path: ".cfdb/queries/self-enrich-git-history.cypher",
                threshold: thresholds::GIT_COVERAGE_THRESHOLD,
                feature_required: FeatureGate::GitEnrich,
                cli_takes_workspace: true,
            },
        ]
    }

    pub fn by_name(name: &str) -> Option<&'static PassDef> {
        Self::all().iter().find(|p| p.name == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn threshold_assignment_matches_rfc_table() {
        let with_threshold: Vec<&str> = PassDef::all()
            .iter()
            .filter(|p| p.threshold.is_some())
            .map(|p| p.name)
            .collect();
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

    #[test]
    fn by_name_lookup() {
        assert_eq!(
            PassDef::by_name("enrich-concepts").map(|p| p.name),
            Some("enrich-concepts")
        );
        assert!(PassDef::by_name("enrich-bogus").is_none());
        assert!(PassDef::by_name("").is_none());
    }

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

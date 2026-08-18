//! The classifier rules, embedded at compile time from `examples/queries/`.
//! The rule files are the user-facing examples and the cross-crate literal
//! pin (`cfdb-eval/tests/conversion_prefix_pin.rs`) reads one of them from
//! that path, so they are not copied here.

/// hsb-by-name — seeds `canonical_candidates` from Pattern A horizontal
/// split-brain findings.
pub(crate) const HSB_BY_NAME_CYPHER: &str =
    include_str!("../../../examples/queries/hsb-by-name.cypher");

/// The six-class classifier rules (RFC-cfdb §A2.1). Each projects
/// `Finding`-compatible columns (qname, name, kind, crate, file, line,
/// bounded_context) and accepts a single `$context` parameter.
pub(crate) const CLASSIFIER_DUPLICATED_FEATURE_CYPHER: &str =
    include_str!("../../../examples/queries/classifier-duplicated-feature.cypher");
pub(crate) const CLASSIFIER_CONTEXT_HOMONYM_CYPHER: &str =
    include_str!("../../../examples/queries/classifier-context-homonym.cypher");
pub(crate) const CLASSIFIER_UNFINISHED_REFACTOR_CYPHER: &str =
    include_str!("../../../examples/queries/classifier-unfinished-refactor.cypher");
pub(crate) const CLASSIFIER_RANDOM_SCATTERING_CYPHER: &str =
    include_str!("../../../examples/queries/classifier-random-scattering.cypher");
pub(crate) const CLASSIFIER_CANONICAL_BYPASS_CYPHER: &str =
    include_str!("../../../examples/queries/classifier-canonical-bypass.cypher");
pub(crate) const CLASSIFIER_UNWIRED_CYPHER: &str =
    include_str!("../../../examples/queries/classifier-unwired.cypher");
/// Production-only variant of the Unwired rule: reads
/// `:Item.reachable_from_production_entry` instead of
/// `:Item.reachable_from_entry`. Selected by `ScopeOptions::production_only`.
pub(crate) const CLASSIFIER_UNWIRED_PRODUCTION_CYPHER: &str =
    include_str!("../../../examples/queries/classifier-unwired-production.cypher");

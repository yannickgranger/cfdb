//! CI-policy thresholds for the 4 ratio-based dogfood invariants.
//!
//! Per RFC-039 §3.2: consts live in this leaf binary (`tools/dogfood-enrich`,
//! `Ca = 0`). They are NOT in `cfdb-core` (clean-arch: inner ring must not
//! know about CI policy) and NOT in `cfdb-cli` (solid-architect SAP: highly
//! efferent crate, unsuitable for stable policy values).
//!
//! Per `CLAUDE.md` §6 row 5 ("No metric ratchets"): tightening is a
//! separate reviewed PR. No baseline file, no allowlist file. A PR that
//! adds one is rejected on sight.
//!
//! Three of the seven passes (deprecation, rfc-docs, concepts) use
//! hard-equality / count-equality sentinels rather than ratio thresholds —
//! see `crate::passes::PassDef::threshold` for the per-pass mapping.

/// `enrich-bounded-context`: ≥N% of `:Item` have non-null
/// `bounded_context` after the combined extract+enrich pipeline.
/// RFC-039 §3.1 (`enrich-bounded-context` row) / §7.4 AC-1.
///
/// Initial floor: 95%. Per-pass issue (#345) sets the real value if
/// cfdb-self requires a different floor.
pub const MIN_BC_COVERAGE_PCT: u32 = 95;

/// `enrich-reachability`: ≥N% of `:Item{kind:Fn}` reachable from any
/// `:EntryPoint` over `CALLS*`. Nightly-only (requires `hir`).
/// RFC-039 §3.1 (`enrich-reachability` row) / §7.6 AC-1.
///
/// Initial floor: 80%. Lower than the other ratios because reachability
/// from extracted entry points is structurally noisier (test-only fns,
/// platform-gated cfg branches, dead-but-public utility code).
pub const MIN_REACHABILITY_PCT: u32 = 80;

/// `enrich-metrics`: ≥N% of `:Item{kind:Fn}` have non-null `cyclomatic`
/// AND `unwrap_count`. Nightly-only (requires `quality-metrics`).
/// RFC-039 §3.1 (`enrich-metrics` row) / §7.7 AC-1.
///
/// Initial floor: 95%. The 5% slack covers macro-expanded fns whose
/// `:Item.file` does not point at a re-parseable `.rs` file.
pub const MIN_METRICS_COVERAGE_PCT: u32 = 95;

/// `enrich-git-history`: ≥N% of `:Item` have non-null
/// `git_last_commit_unix_ts` (the actual emitted attribute name —
/// `commit_age_days` from the R1 RFC draft does not exist; rust-systems
/// caught the typo). Nightly-only (requires `git-enrich`).
/// RFC-039 §3.1 (`enrich-git-history` row) / §7.8 AC-1.
///
/// Initial floor: 95%. The slack covers items whose `file` is outside
/// the git tree (vendored deps, generated code).
pub const MIN_GIT_COVERAGE_PCT: u32 = 95;

#[cfg(test)]
mod tests {
    use super::*;

    /// Pin initial floor values + assert percentage bounds in one test.
    /// A careless edit triggers a test failure (not a silent ratchet),
    /// and a typo setting one to 250 fails the bounds check.
    #[test]
    fn thresholds_pin_initial_values_and_are_valid_percentages() {
        for (name, value, expected) in [
            ("MIN_BC_COVERAGE_PCT", MIN_BC_COVERAGE_PCT, 95),
            ("MIN_REACHABILITY_PCT", MIN_REACHABILITY_PCT, 80),
            ("MIN_METRICS_COVERAGE_PCT", MIN_METRICS_COVERAGE_PCT, 95),
            ("MIN_GIT_COVERAGE_PCT", MIN_GIT_COVERAGE_PCT, 95),
        ] {
            assert_eq!(value, expected, "{name} initial floor moved");
            assert!(
                value <= 100,
                "{name} = {value} is not a valid percentage (>100)"
            );
        }
    }
}

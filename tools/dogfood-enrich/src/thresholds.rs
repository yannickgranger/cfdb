//! CI-policy thresholds for the 7 dogfood invariants.
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
//! Each of the 7 passes carries a `pub const … _THRESHOLD: Option<u32>`.
//! Four are `Some(N)` (the ratio-based passes); three are `None`
//! sentinels for the hard-equality / count-equality passes (deprecation,
//! rfc-docs, concepts). The `Option` shape lets per-pass code reference
//! the const uniformly via `passes::PassDef::threshold` without special-
//! casing.

/// `enrich-deprecation` — hard-equality sentinel: extracted
/// `:Item.is_deprecated` count must equal grep'd `#[deprecated]` count.
/// `None` means "no ratio threshold; the per-pass query asserts equality."
/// RFC-039 §3.1 (`enrich-deprecation` row) / §7.2.
pub const DEPRECATION_THRESHOLD: Option<u32> = None;

/// `enrich-rfc-docs` — hard-equality sentinel: `:RfcDoc` count must
/// match `docs/RFC-*.md` count AND `:REFERENCED_BY` edges must be > 0.
/// RFC-039 §3.1 (`enrich-rfc-docs` row) / §7.3.
pub const RFC_DOCS_THRESHOLD: Option<u32> = None;

/// `enrich-bounded-context`: ≥N% of `:Item` have non-null
/// `bounded_context` after the combined extract+enrich pipeline.
/// RFC-039 §3.1 (`enrich-bounded-context` row) / §7.4.
///
/// Initial floor: 95%. Per-pass issue (#345) sets the real value if
/// cfdb-self requires a different floor.
pub const BC_COVERAGE_THRESHOLD: Option<u32> = Some(95);

/// `enrich-concepts` — hard-equality sentinel: `:Concept` count must
/// equal distinct context names across all `.cfdb/concepts/*.toml`,
/// `:LABELED_AS` > 0, conditional `:CANONICAL_FOR` > 0.
/// RFC-039 §3.1 + §3.1.2 / §7.5.
pub const CONCEPTS_THRESHOLD: Option<u32> = None;

/// `enrich-reachability`: ≥N% of `:Item{kind:Fn}` reachable from any
/// `:EntryPoint` over `CALLS*`. Nightly-only (requires `hir`).
/// RFC-039 §3.1 (`enrich-reachability` row) / §7.6.
///
/// Initial floor: 80%. Lower than the other ratios because reachability
/// from extracted entry points is structurally noisier (test-only fns,
/// platform-gated cfg branches, dead-but-public utility code).
pub const REACHABILITY_THRESHOLD: Option<u32> = Some(80);

/// `enrich-metrics`: ≥N% of `:Item{kind:Fn}` have non-null `cyclomatic`
/// AND `unwrap_count`. Nightly-only (requires `quality-metrics`).
/// RFC-039 §3.1 (`enrich-metrics` row) / §7.7.
///
/// Initial floor: 95%. The 5% slack covers macro-expanded fns whose
/// `:Item.file` does not point at a re-parseable `.rs` file.
pub const METRICS_COVERAGE_THRESHOLD: Option<u32> = Some(95);

/// `enrich-git-history`: ≥N% of `:Item` have non-null
/// `git_last_commit_unix_ts` (the actual emitted attribute name —
/// `commit_age_days` from the R1 RFC draft does not exist; rust-systems
/// caught the typo). Nightly-only (requires `git-enrich`).
/// RFC-039 §3.1 (`enrich-git-history` row) / §7.8.
///
/// Initial floor: 95%. The slack covers items whose `file` is outside
/// the git tree (vendored deps, generated code).
pub const GIT_COVERAGE_THRESHOLD: Option<u32> = Some(95);

#[cfg(test)]
mod tests {
    use super::*;

    /// Pin initial values, assert hard-equality sentinels are `None`,
    /// and assert ratio thresholds are valid percentages, in one test.
    /// A careless edit triggers a test failure (not a silent ratchet),
    /// and a typo setting one to 250 fails the bounds check.
    #[test]
    fn thresholds_pin_initial_values_and_are_valid_percentages() {
        // Hard-equality passes: sentinel must remain None.
        for (name, value) in [
            ("DEPRECATION_THRESHOLD", DEPRECATION_THRESHOLD),
            ("RFC_DOCS_THRESHOLD", RFC_DOCS_THRESHOLD),
            ("CONCEPTS_THRESHOLD", CONCEPTS_THRESHOLD),
        ] {
            assert!(
                value.is_none(),
                "{name} should be None (hard-equality sentinel) but is {value:?}"
            );
        }
        // Ratio passes: pin values + assert percentage bounds.
        for (name, value, expected) in [
            ("BC_COVERAGE_THRESHOLD", BC_COVERAGE_THRESHOLD, 95),
            ("REACHABILITY_THRESHOLD", REACHABILITY_THRESHOLD, 80),
            ("METRICS_COVERAGE_THRESHOLD", METRICS_COVERAGE_THRESHOLD, 95),
            ("GIT_COVERAGE_THRESHOLD", GIT_COVERAGE_THRESHOLD, 95),
        ] {
            let v = value.expect("ratio threshold must be Some");
            assert_eq!(v, expected, "{name} initial floor moved");
            assert!(v <= 100, "{name} = {v} is not a valid percentage");
        }
    }
}

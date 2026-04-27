//! `check-prelude-triggers` — Tier-1 mechanical C-trigger binary per RFC-034 v3.3 §4.2.
//!
//! Fires 5 deterministic triggers against a workspace diff:
//!
//! | Trigger | Concept |
//! |---|---|
//! | C1 | cross-context change — diff touches ≥2 bounded contexts per `context-map.toml` |
//! | C3 | port trait signature — diff touches a file under `crates/ports*/src/` |
//! | C7 | financial-precision path — diff touches a crate listed in `financial-precision-crates.toml` |
//! | C8 | pipeline-stage cross — diff touches ≥2 stages per `pipeline-stages.toml` |
//! | C9 | workspace cardinality — workspace `Cargo.toml` is in the diff |
//!
//! The binary is stateless: it reads argv-supplied paths and emits a versioned
//! JSON envelope on stdout. See [`report::PreludeTriggerReport`] for the shape.
//!
//! Homonym note: the `C*` IDs here are RFC-034 mechanical pre-council triggers
//! and are distinct from cfdb's internal Cypher-query triggers `T1`/`T3`
//! (cfdb issues #100/#101). This binary does NOT implement T-triggers.

pub mod report;
pub mod toml_io;
pub mod trigger_id;
pub mod triggers;

pub use report::PreludeTriggerReport;
pub use toml_io::LoadError;
pub use trigger_id::TriggerId;

/// Stderr message emitted when `--require-fresh` rejects a stale envelope.
///
/// Contract is stable: skill-side consumers (`/discover`, `/ship` pre-flight)
/// match against this prefix to distinguish freshness rejections from other
/// usage-class exit-1 failures.
pub const STALE_REFS_MESSAGE: &str =
    "from_ref equals to_ref; refresh required (RFC-034 §4.2 lower-bound semantic)";

/// Validate that the diff is non-empty when the consumer demanded a fresh capture.
///
/// Used by `main()` before subcommand dispatch. When `require_fresh` is set
/// AND `from_ref == to_ref`, returns `Err` carrying [`STALE_REFS_MESSAGE`];
/// the caller emits the message on stderr and exits with code 1 (usage class).
///
/// When `require_fresh` is unset, the function is a no-op — the binary's
/// pre-`/ship` `from_ref == to_ref` capture is still a valid use case for
/// snapshot-taking at issue start. The flag opts a consumer into the stricter
/// "this envelope must reflect a real diff" contract.
pub fn validate_freshness(
    require_fresh: bool,
    from_ref: &str,
    to_ref: &str,
) -> Result<(), &'static str> {
    if require_fresh && from_ref == to_ref {
        return Err(STALE_REFS_MESSAGE);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_freshness_rejects_equal_refs_when_required() {
        let result = validate_freshness(true, "abc123", "abc123");
        assert_eq!(result, Err(STALE_REFS_MESSAGE));
    }

    #[test]
    fn validate_freshness_accepts_distinct_refs_when_required() {
        let result = validate_freshness(true, "develop", "feature-tip");
        assert_eq!(result, Ok(()));
    }

    #[test]
    fn validate_freshness_is_noop_when_flag_unset() {
        // Default behavior — issue-start snapshot capture (from_ref == to_ref)
        // remains a valid use case until the consumer opts in to strictness.
        let result = validate_freshness(false, "abc123", "abc123");
        assert_eq!(result, Ok(()));
    }
}

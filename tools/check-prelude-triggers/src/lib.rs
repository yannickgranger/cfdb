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

use std::path::Path;

/// Run all 5 C-triggers against the same diff snapshot and return one
/// consolidated [`PreludeTriggerReport`].
///
/// Calls the same pure `triggers::*::run()` functions as the per-trigger
/// subcommands — there is no shell-out and no parallel evaluation. The 5
/// evaluators are pure-functional and cheap; one `all` invocation either
/// succeeds atomically or fails with the worst exit code (the first error
/// wins).
///
/// Per-trigger evidence is written into the report's `evidence` map under
/// every trigger's ID, regardless of whether the trigger fired — fired
/// triggers also appear in `triggers_fired` (sorted, deduped). This is the
/// same shape the per-trigger subcommands emit individually; consumers
/// merging 5 separate envelopes get exactly the same result. The `all`
/// subcommand exists to remove that merge step.
///
/// # Errors
/// Returns the first [`LoadError`] encountered — whichever evaluator fails
/// first short-circuits the run. The order is C1 → C3 → C7 → C8 → C9.
pub fn run_all(
    context_map: &Path,
    financial_precision_crates: &Path,
    pipeline_stages: &Path,
    workspace_root: &Path,
    changed_paths: &Path,
    from_ref: String,
    to_ref: String,
) -> Result<PreludeTriggerReport, LoadError> {
    let outcomes = [
        (
            TriggerId::C1,
            triggers::c1_cross_context::run(context_map, changed_paths)?,
        ),
        (
            TriggerId::C3,
            triggers::c3_port_signature::run(changed_paths)?,
        ),
        (
            TriggerId::C7,
            triggers::c7_financial_precision::run(financial_precision_crates, changed_paths)?,
        ),
        (
            TriggerId::C8,
            triggers::c8_pipeline_stage::run(pipeline_stages, changed_paths)?,
        ),
        (
            TriggerId::C9,
            triggers::c9_workspace_cardinality::run(workspace_root, changed_paths)?,
        ),
    ];

    let mut report = PreludeTriggerReport::new(from_ref, to_ref);
    for (id, outcome) in outcomes {
        if outcome.fired {
            report.record(id, outcome.evidence);
        } else {
            // Same shape as main() emits per-trigger: un-fired triggers still
            // contribute evidence so consumers see what was checked.
            report
                .evidence
                .insert(id.as_str().to_string(), outcome.evidence);
        }
    }
    Ok(report)
}

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
    use std::fs;
    use tempfile::tempdir;

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

    /// Build a synthetic fixture root with all 4 input TOMLs + a Cargo.toml +
    /// a changed-paths file. Fixture content is the minimum needed to make
    /// the chosen triggers fire.
    fn fire_fixture(
        root: &std::path::Path,
        changed: &str,
    ) -> (
        std::path::PathBuf,
        std::path::PathBuf,
        std::path::PathBuf,
        std::path::PathBuf,
        std::path::PathBuf,
    ) {
        let context_map = root.join("context-map.toml");
        fs::write(
            &context_map,
            r#"
            [contexts.trading]
            path_prefixes = ["crates/domain-trading/"]
            [contexts.risk]
            path_prefixes = ["crates/domain-risk/"]
            "#,
        )
        .expect("write context-map");

        let financial = root.join("financial-precision-crates.toml");
        fs::write(
            &financial,
            r#"financial_precision_prefixes = ["crates/domain-trading/"]"#,
        )
        .expect("write financial");

        let stages = root.join("pipeline-stages.toml");
        fs::write(
            &stages,
            r#"
            [stages.signal]
            path_prefixes = ["crates/domain-trading/"]
            [stages.execution]
            path_prefixes = ["crates/domain-risk/"]
            "#,
        )
        .expect("write stages");

        let workspace_root = root.to_path_buf();
        fs::write(
            workspace_root.join("Cargo.toml"),
            r#"
            [workspace]
            resolver = "2"
            members = ["crates/domain-trading", "crates/domain-risk"]
            "#,
        )
        .expect("write Cargo.toml");

        let changed_paths = root.join("changed.txt");
        fs::write(&changed_paths, changed).expect("write changed-paths");

        (
            context_map,
            financial,
            stages,
            workspace_root,
            changed_paths,
        )
    }

    #[test]
    fn run_all_fires_multiple_triggers_on_real_money_path_diff() {
        // Diff touches both `domain-trading` and `domain-risk` AND the
        // workspace Cargo.toml — should fire C1 (cross-context),
        // C7 (financial-precision), C8 (pipeline-stage cross), C9 (cardinality).
        let dir = tempdir().expect("tempdir");
        let changed = "\
crates/domain-trading/src/order.rs
crates/domain-risk/src/limit.rs
Cargo.toml
";
        let (cm, fin, st, ws, cp) = fire_fixture(dir.path(), changed);

        let report = run_all(&cm, &fin, &st, &ws, &cp, "develop".into(), "tip".into())
            .expect("run_all succeeds");

        assert_eq!(report.from_ref, "develop");
        assert_eq!(report.to_ref, "tip");
        assert!(report.triggers_fired.contains(&TriggerId::C1));
        assert!(report.triggers_fired.contains(&TriggerId::C7));
        assert!(report.triggers_fired.contains(&TriggerId::C8));
        assert!(report.triggers_fired.contains(&TriggerId::C9));
        // C3 doesn't fire — no `crates/ports*/src/` paths.
        assert!(!report.triggers_fired.contains(&TriggerId::C3));
        // All 5 evidence entries present regardless of fired status.
        for id in ["C1", "C3", "C7", "C8", "C9"] {
            assert!(
                report.evidence.contains_key(id),
                "evidence missing trigger {id}: {:?}",
                report.evidence.keys().collect::<Vec<_>>()
            );
        }
        // Sort + dedup invariant from PreludeTriggerReport::record holds.
        let mut sorted = report.triggers_fired.clone();
        sorted.sort();
        assert_eq!(sorted, report.triggers_fired);
    }

    #[test]
    fn run_all_emits_empty_triggers_fired_on_no_match_diff() {
        let dir = tempdir().expect("tempdir");
        // Diff touches only docs — no trigger should fire.
        let (cm, fin, st, ws, cp) = fire_fixture(dir.path(), "docs/README.md\n");

        let report = run_all(&cm, &fin, &st, &ws, &cp, "develop".into(), "tip".into())
            .expect("run_all succeeds");

        assert!(
            report.triggers_fired.is_empty(),
            "no triggers should fire on docs-only diff; got: {:?}",
            report.triggers_fired
        );
        // Evidence map still populated for all 5 — consumers see what was checked.
        for id in ["C1", "C3", "C7", "C8", "C9"] {
            assert!(
                report.evidence.contains_key(id),
                "evidence missing trigger {id}",
            );
        }
    }
}

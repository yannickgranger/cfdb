pub mod report;
pub mod toml_io;
pub mod trigger_id;
pub mod triggers;

pub use report::PreludeTriggerReport;
pub use toml_io::LoadError;
pub use trigger_id::TriggerId;

use std::path::Path;

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
            report
                .evidence
                .insert(id.as_str().to_string(), outcome.evidence);
        }
    }
    Ok(report)
}

pub const STALE_REFS_MESSAGE: &str =
    "from_ref equals to_ref; refresh required (RFC-034 §4.2 lower-bound semantic)";

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
        let result = validate_freshness(false, "abc123", "abc123");
        assert_eq!(result, Ok(()));
    }

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
        assert!(!report.triggers_fired.contains(&TriggerId::C3));
        for id in ["C1", "C3", "C7", "C8", "C9"] {
            assert!(
                report.evidence.contains_key(id),
                "evidence missing trigger {id}: {:?}",
                report.evidence.keys().collect::<Vec<_>>()
            );
        }
        let mut sorted = report.triggers_fired.clone();
        sorted.sort();
        assert_eq!(sorted, report.triggers_fired);
    }

    #[test]
    fn run_all_emits_empty_triggers_fired_on_no_match_diff() {
        let dir = tempdir().expect("tempdir");
        let (cm, fin, st, ws, cp) = fire_fixture(dir.path(), "docs/README.md\n");

        let report = run_all(&cm, &fin, &st, &ws, &cp, "develop".into(), "tip".into())
            .expect("run_all succeeds");

        assert!(
            report.triggers_fired.is_empty(),
            "no triggers should fire on docs-only diff; got: {:?}",
            report.triggers_fired
        );
        for id in ["C1", "C3", "C7", "C8", "C9"] {
            assert!(
                report.evidence.contains_key(id),
                "evidence missing trigger {id}",
            );
        }
    }
}

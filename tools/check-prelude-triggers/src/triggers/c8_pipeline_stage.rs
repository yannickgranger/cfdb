//! C8 — pipeline-stage cross detection.
//!
//! Fires when a changed path set touches ≥2 stages in `pipeline-stages.toml`
//! (`[stages.signal]`, `[stages.sizing]`, `[stages.execution]`, ...). The RFC
//! treats a single change crossing stage boundaries as a candidate split-brain
//! bypass — pre-council inspection is mandatory.

use serde_json::json;
use std::collections::BTreeMap;
use std::path::Path;

use crate::toml_io::{read_changed_paths, read_toml, LoadError};
use crate::triggers::TriggerOutcome;

/// Run the C8 check against the on-disk inputs.
///
/// # Errors
/// Returns [`LoadError`] if either file is missing or malformed.
pub fn run(pipeline_stages: &Path, changed_paths: &Path) -> Result<TriggerOutcome, LoadError> {
    let cfg = read_toml(pipeline_stages)?;
    let changed = read_changed_paths(changed_paths)?;
    Ok(evaluate(&cfg, &changed))
}

/// Pure evaluator exposed for unit tests.
#[must_use]
pub fn evaluate(pipeline_cfg: &toml::Value, changed_paths: &[String]) -> TriggerOutcome {
    let stages = pipeline_cfg.get("stages").and_then(toml::Value::as_table);

    let Some(stages) = stages else {
        return TriggerOutcome {
            fired: false,
            evidence: json!({
                "stages_touched": [],
                "rule": "pipeline-stages.toml missing [stages] table",
            }),
        };
    };

    let mut touched: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (stage_name, stage_def) in stages {
        let Some(prefixes) = stage_def
            .get("path_prefixes")
            .and_then(toml::Value::as_array)
        else {
            continue;
        };
        let prefixes: Vec<&str> = prefixes.iter().filter_map(toml::Value::as_str).collect();
        for path in changed_paths {
            if prefixes.iter().any(|p| path.starts_with(p)) {
                touched
                    .entry(stage_name.clone())
                    .or_default()
                    .push(path.clone());
            }
        }
    }

    let stages_touched: Vec<String> = touched.keys().cloned().collect();
    let fired = stages_touched.len() >= 2;

    TriggerOutcome {
        fired,
        evidence: json!({
            "stages_touched": stages_touched,
            "matches": touched,
            "rule": "pipeline-stages.toml (>=2 stages touched fires)",
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::evaluate;

    fn cfg() -> toml::Value {
        toml::from_str(
            r#"
            [stages.signal]
            path_prefixes = ["crates/domain-strategy/"]

            [stages.execution]
            path_prefixes = ["crates/ports-trading-execution/"]
            "#,
        )
        .unwrap()
    }

    #[test]
    fn c8_fires_on_stage_cross() {
        let changed = vec![
            "crates/domain-strategy/src/signal.rs".to_string(),
            "crates/ports-trading-execution/src/lib.rs".to_string(),
        ];
        let out = evaluate(&cfg(), &changed);
        assert!(out.fired);
    }

    #[test]
    fn c8_stays_silent_on_single_stage() {
        let changed = vec!["crates/domain-strategy/src/signal.rs".to_string()];
        let out = evaluate(&cfg(), &changed);
        assert!(!out.fired);
    }
}

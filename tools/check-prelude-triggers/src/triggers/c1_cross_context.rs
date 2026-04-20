//! C1 — cross-context change detection.
//!
//! Fires when a changed-path list touches ≥2 bounded contexts declared in
//! `context-map.toml`. Mechanism: bucket each changed path against every
//! context's `path_prefixes` list; a path that matches at least one prefix
//! is credited to that context.
//!
//! Reference: RFC-034 §4.2 table + POC stub at
//! `qbot-core:poc/prevention-round-1:docs/poc/prevention/round-4/bin/check-prelude-triggers.py`.

use serde_json::json;
use std::collections::BTreeMap;
use std::path::Path;

use crate::toml_io::{read_changed_paths, read_toml, LoadError};
use crate::triggers::TriggerOutcome;

/// Run the C1 check against the on-disk inputs.
///
/// # Errors
/// Returns a [`LoadError`] if either file is missing or malformed.
pub fn run(context_map: &Path, changed_paths: &Path) -> Result<TriggerOutcome, LoadError> {
    let map = read_toml(context_map)?;
    let changed = read_changed_paths(changed_paths)?;
    Ok(evaluate(&map, &changed))
}

/// Pure evaluator exposed for unit tests.
#[must_use]
pub fn evaluate(context_map: &toml::Value, changed_paths: &[String]) -> TriggerOutcome {
    let contexts = context_map.get("contexts").and_then(toml::Value::as_table);

    let Some(contexts) = contexts else {
        return TriggerOutcome {
            fired: false,
            evidence: json!({
                "contexts_touched": [],
                "rule": "context-map.toml missing [contexts] table",
            }),
        };
    };

    // context -> matched paths
    let mut touched: BTreeMap<String, Vec<String>> = BTreeMap::new();

    for (ctx_name, ctx_def) in contexts {
        let Some(prefixes) = ctx_def.get("path_prefixes").and_then(toml::Value::as_array) else {
            continue;
        };
        let prefixes: Vec<&str> = prefixes.iter().filter_map(toml::Value::as_str).collect();
        for path in changed_paths {
            if prefixes.iter().any(|p| path.starts_with(p)) {
                touched
                    .entry(ctx_name.clone())
                    .or_default()
                    .push(path.clone());
            }
        }
    }

    let touched_contexts: Vec<String> = touched.keys().cloned().collect();
    let fired = touched_contexts.len() >= 2;

    TriggerOutcome {
        fired,
        evidence: json!({
            "contexts_touched": touched_contexts,
            "matches": touched,
            "rule": "context-map.toml (>=2 contexts touched fires)",
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::evaluate;

    fn map() -> toml::Value {
        toml::from_str(
            r#"
            [contexts.trading]
            path_prefixes = ["crates/domain-trading/", "crates/ports-trading/"]

            [contexts.risk]
            path_prefixes = ["crates/domain-risk/"]
            "#,
        )
        .unwrap()
    }

    #[test]
    fn c1_fires_when_two_contexts_touched() {
        let changed = vec![
            "crates/domain-trading/src/order.rs".to_string(),
            "crates/domain-risk/src/limit.rs".to_string(),
        ];
        let out = evaluate(&map(), &changed);
        assert!(out.fired, "C1 should fire on cross-context change");
        let touched = out.evidence["contexts_touched"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect::<Vec<_>>();
        assert_eq!(touched, vec!["risk".to_string(), "trading".to_string()]);
    }

    #[test]
    fn c1_stays_silent_when_single_context_touched() {
        let changed = vec![
            "crates/domain-trading/src/order.rs".to_string(),
            "crates/ports-trading/src/port.rs".to_string(),
        ];
        let out = evaluate(&map(), &changed);
        assert!(!out.fired, "C1 must not fire when only one context touched");
    }
}

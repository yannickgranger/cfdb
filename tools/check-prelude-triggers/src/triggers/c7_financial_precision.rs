//! C7 — financial-precision path change detection.
//!
//! Fires when a changed path sits under any prefix declared in
//! `financial-precision-crates.toml` (top-level `financial_precision_prefixes`
//! array). These are crates where `rust_decimal::Decimal` is mandatory; any
//! touch is a lower-bound signal that the agent must inspect for f64 escapes
//! (per RFC-034 §4.2 lower-bound semantic).

use serde_json::json;
use std::path::Path;

use crate::toml_io::{read_changed_paths, read_toml, LoadError};
use crate::triggers::TriggerOutcome;

/// Run the C7 check against the on-disk inputs.
///
/// # Errors
/// Returns [`LoadError`] if either file is missing or malformed.
pub fn run(fin_precision: &Path, changed_paths: &Path) -> Result<TriggerOutcome, LoadError> {
    let cfg = read_toml(fin_precision)?;
    let changed = read_changed_paths(changed_paths)?;
    Ok(evaluate(&cfg, &changed))
}

/// Pure evaluator exposed for unit tests.
#[must_use]
pub fn evaluate(fin_config: &toml::Value, changed_paths: &[String]) -> TriggerOutcome {
    let prefixes: Vec<&str> = fin_config
        .get("financial_precision_prefixes")
        .and_then(toml::Value::as_array)
        .map(|a| a.iter().filter_map(toml::Value::as_str).collect())
        .unwrap_or_default();

    let matched: Vec<String> = changed_paths
        .iter()
        .filter(|p| prefixes.iter().any(|pref| p.starts_with(pref)))
        .cloned()
        .collect();

    TriggerOutcome {
        fired: !matched.is_empty(),
        evidence: json!({
            "matched_paths": matched,
            "rule": "financial-precision-crates.toml (any match fires)",
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::evaluate;

    fn cfg() -> toml::Value {
        toml::from_str(
            r#"
            financial_precision_prefixes = [
              "crates/domain-trading/",
              "crates/domain-portfolio/",
            ]
            "#,
        )
        .unwrap()
    }

    #[test]
    fn c7_fires_when_financial_crate_touched() {
        let changed = vec!["crates/domain-trading/src/position.rs".to_string()];
        let out = evaluate(&cfg(), &changed);
        assert!(out.fired);
    }

    #[test]
    fn c7_stays_silent_when_non_financial_crate_touched() {
        let changed = vec!["crates/qbot-mcp/src/handler.rs".to_string()];
        let out = evaluate(&cfg(), &changed);
        assert!(!out.fired);
    }
}

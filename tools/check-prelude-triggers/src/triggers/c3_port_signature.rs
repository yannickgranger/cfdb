//! C3 — port trait signature change detection.
//!
//! Fires when a changed path matches `^crates/ports[^/]*/src/`. Port trait
//! signatures carry cross-context contract guarantees; any change belongs in
//! pre-council review per RFC-034 §4.2.
//!
//! Mechanism: regex over each changed path. No TOML input needed.

use regex::Regex;
use serde_json::json;
use std::path::Path;
use std::sync::OnceLock;

use crate::toml_io::{read_changed_paths, LoadError};
use crate::triggers::TriggerOutcome;

/// RFC-034 §4.2 regex: matches any file under a `crates/ports*/src/` tree.
/// The pattern is anchored at the start of the path so nested occurrences
/// (e.g. a fixture living inside a `ports-*` crate's `tests/`) do not fire.
fn port_regex() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^crates/ports[^/]*/src/").expect("valid port regex"))
}

/// Run the C3 check against the changed-paths file.
///
/// # Errors
/// Returns [`LoadError`] if the changed-paths file cannot be read.
pub fn run(changed_paths: &Path) -> Result<TriggerOutcome, LoadError> {
    let changed = read_changed_paths(changed_paths)?;
    Ok(evaluate(&changed))
}

/// Pure evaluator exposed for unit tests.
#[must_use]
pub fn evaluate(changed_paths: &[String]) -> TriggerOutcome {
    let re = port_regex();
    let matched: Vec<String> = changed_paths
        .iter()
        .filter(|p| re.is_match(p))
        .cloned()
        .collect();
    TriggerOutcome {
        fired: !matched.is_empty(),
        evidence: json!({
            "matched_paths": matched,
            "rule": "^crates/ports[^/]*/src/",
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::evaluate;

    #[test]
    fn c3_fires_on_port_src_change() {
        let changed = vec!["crates/ports-trading/src/executor.rs".to_string()];
        let out = evaluate(&changed);
        assert!(out.fired, "C3 should fire on ports-*/src/*.rs change");
    }

    #[test]
    fn c3_stays_silent_on_non_port_change() {
        let changed = vec!["crates/domain-trading/src/order.rs".to_string()];
        let out = evaluate(&changed);
        assert!(!out.fired, "C3 must not fire on domain-only change");
    }

    #[test]
    fn c3_does_not_fire_on_ports_tests_dir() {
        let changed = vec!["crates/ports-trading/tests/smoke.rs".to_string()];
        let out = evaluate(&changed);
        assert!(
            !out.fired,
            "C3 anchored on src/ should not fire on tests/ change"
        );
    }
}

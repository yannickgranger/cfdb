use serde_json::json;
use std::path::Path;

use crate::toml_io::{read_changed_paths, read_toml, LoadError};
use crate::triggers::TriggerOutcome;

pub fn run(workspace_root: &Path, changed_paths: &Path) -> Result<TriggerOutcome, LoadError> {
    let changed = read_changed_paths(changed_paths)?;
    let cargo_touched = changed.iter().any(|p| p == "Cargo.toml");
    if !cargo_touched {
        return Ok(evaluate_absent(&changed));
    }
    let manifest_path = workspace_root.join("Cargo.toml");
    let manifest = read_toml(&manifest_path)?;
    Ok(evaluate_present(&manifest, &changed))
}

#[must_use]
pub fn evaluate_absent(changed_paths: &[String]) -> TriggerOutcome {
    TriggerOutcome {
        fired: false,
        evidence: json!({
            "cargo_toml_touched": false,
            "changed_count": changed_paths.len(),
            "rule": "workspace Cargo.toml not in diff",
        }),
    }
}

#[must_use]
pub fn evaluate_present(manifest: &toml::Value, changed_paths: &[String]) -> TriggerOutcome {
    let members: Vec<String> = manifest
        .get("workspace")
        .and_then(toml::Value::as_table)
        .and_then(|w| w.get("members"))
        .and_then(toml::Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(toml::Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();

    TriggerOutcome {
        fired: true,
        evidence: json!({
            "cargo_toml_touched": true,
            "workspace_members_count": members.len(),
            "workspace_members": members,
            "changed_count": changed_paths.len(),
            "rule": "workspace Cargo.toml in diff — cardinality reported",
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::{evaluate_absent, evaluate_present};

    #[test]
    fn c9_fires_when_workspace_cargo_toml_in_diff() {
        let manifest: toml::Value = toml::from_str(
            r#"
            [workspace]
            resolver = "2"
            members = ["crates/a", "crates/b", "crates/c"]
            "#,
        )
        .unwrap();
        let changed = vec!["Cargo.toml".to_string()];
        let out = evaluate_present(&manifest, &changed);
        assert!(out.fired);
        assert_eq!(out.evidence["workspace_members_count"].as_u64(), Some(3));
    }

    #[test]
    fn c9_stays_silent_when_cargo_toml_not_in_diff() {
        let changed = vec!["crates/domain-trading/src/order.rs".to_string()];
        let out = evaluate_absent(&changed);
        assert!(!out.fired);
        assert_eq!(out.evidence["cargo_toml_touched"].as_bool(), Some(false));
    }
}

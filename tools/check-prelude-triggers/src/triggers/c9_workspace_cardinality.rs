//! C9 — workspace cardinality change detection.
//!
//! Fires when the workspace root `Cargo.toml` appears in the diff AND the
//! current `[workspace] members = [...]` list parses successfully. This is the
//! RFC-034 §4.2 lower-bound signal that the workspace is gaining or losing a
//! crate in the changeset; cardinality growth beyond a threshold triggers
//! pre-council review.
//!
//! Rust-systems Amendment 2 (RFC-034 §4.2): `Cargo.toml` is parsed directly
//! via the `toml` crate. The binary MUST NOT invoke `cargo` as a subprocess
//! nor depend on `cargo_metadata`, which would re-enter workspace lock
//! resolution and deadlock in nested CI contexts.

use serde_json::json;
use std::path::Path;

use crate::toml_io::{read_changed_paths, read_toml, LoadError};
use crate::triggers::TriggerOutcome;

/// Run the C9 check against the on-disk inputs.
///
/// `workspace_root` points at the directory containing the root `Cargo.toml`;
/// the binary parses `<workspace_root>/Cargo.toml` directly.
///
/// # Errors
/// Returns [`LoadError`] if the changed-paths file cannot be read OR if the
/// workspace `Cargo.toml` is referenced by the diff but cannot be parsed.
pub fn run(workspace_root: &Path, changed_paths: &Path) -> Result<TriggerOutcome, LoadError> {
    let changed = read_changed_paths(changed_paths)?;
    // Only parse the Cargo.toml when the diff actually touches it — avoids
    // surfacing unrelated parse errors when C9 is being run on a non-manifest
    // change.
    let cargo_touched = changed.iter().any(|p| p == "Cargo.toml");
    if !cargo_touched {
        return Ok(evaluate_absent(&changed));
    }
    let manifest_path = workspace_root.join("Cargo.toml");
    let manifest = read_toml(&manifest_path)?;
    Ok(evaluate_present(&manifest, &changed))
}

/// Evaluation branch when `Cargo.toml` is NOT in the changed-paths list.
/// C9 is silent and evidence records the rule.
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

/// Evaluation branch when `Cargo.toml` is in the changed-paths list. Parses
/// `workspace.members`, fires, and reports the current count.
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

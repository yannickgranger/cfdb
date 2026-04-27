//! I5.1 feature-presence guard per RFC-039 §4.
//!
//! Before running the dogfood sentinel for a feature-gated pass, the
//! harness invokes `cfdb enrich-<pass>` and inspects `EnrichReport.ran`.
//! When `ran == false` (the off-feature dispatch path at
//! `crates/cfdb-petgraph/src/enrich_backend.rs:178-262`), the harness
//! exits with `EXIT_RUNTIME_ERROR` and a "feature missing" message —
//! NOT with the sentinel result, because a binary built without the
//! feature would silently report 100% null coverage and look like a
//! real regression.

use std::io;
use std::path::Path;
use std::process::Command;

use serde::Deserialize;
use thiserror::Error;

/// The subset of `cfdb_core::enrich::EnrichReport` we care about. Kept
/// local rather than imported from `cfdb-core` because the JSON wire
/// form is the contract — depending on a Rust struct would couple this
/// harness to in-process refactors of `EnrichReport` that don't change
/// the wire shape.
#[derive(Debug, Deserialize)]
struct EnrichReportSubset {
    ran: bool,
    #[serde(default)]
    warnings: Vec<String>,
}

#[derive(Debug, Error)]
pub enum GuardError {
    #[error("failed to invoke {binary}: {source}")]
    Spawn {
        binary: String,
        #[source]
        source: io::Error,
    },
    #[error("subprocess {binary} terminated by signal")]
    Signal { binary: String },
    #[error("subprocess {binary} exited {exit}; stderr: {stderr}")]
    NonZeroExit {
        binary: String,
        exit: i32,
        stderr: String,
    },
    #[error("failed to parse EnrichReport JSON from {binary}: {source}\nstdout: {stdout}")]
    JsonParse {
        binary: String,
        stdout: String,
        #[source]
        source: serde_json::Error,
    },
    #[error(
        "feature missing: cfdb enrich-{pass} reported ran=false. \
         Rebuild cfdb-cli with the matching feature flag (hir, quality-metrics, or git-enrich) \
         per RFC-039 §I5.1. Warnings from cfdb: {warnings:?}"
    )]
    FeatureMissing { pass: String, warnings: Vec<String> },
}

/// Parse an `EnrichReport`-shaped JSON string into `(ran, warnings)`.
///
/// Returned tuple lets the binary surface off-feature warnings in
/// the `FeatureMissing` error message and lets tests assert both
/// fields independently.
pub fn parse_report(json: &str) -> Result<(bool, Vec<String>), serde_json::Error> {
    let report: EnrichReportSubset = serde_json::from_str(json)?;
    Ok((report.ran, report.warnings))
}

/// Invoke `cfdb enrich-<pass>` against the keyspace and verify the
/// pass actually ran. Returns `Ok(())` if `ran == true`. Returns
/// `Err(GuardError::FeatureMissing)` if `ran == false`. Other errors
/// bubble as runtime failures.
pub fn check_pass_ran(
    cfdb_bin: &Path,
    pass_name: &str,
    db: &Path,
    keyspace: &str,
    workspace: Option<&Path>,
) -> Result<(), GuardError> {
    let mut cmd = Command::new(cfdb_bin);
    cmd.arg(pass_name)
        .arg("--db")
        .arg(db)
        .arg("--keyspace")
        .arg(keyspace);
    if let Some(ws) = workspace {
        cmd.arg("--workspace").arg(ws);
    }
    let output = cmd.output().map_err(|source| GuardError::Spawn {
        binary: cfdb_bin.display().to_string(),
        source,
    })?;
    let exit = output.status.code().ok_or_else(|| GuardError::Signal {
        binary: cfdb_bin.display().to_string(),
    })?;
    if exit != 0 {
        return Err(GuardError::NonZeroExit {
            binary: cfdb_bin.display().to_string(),
            exit,
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let (ran, warnings) = parse_report(&stdout).map_err(|source| GuardError::JsonParse {
        binary: cfdb_bin.display().to_string(),
        stdout: stdout.clone().into_owned(),
        source,
    })?;
    if !ran {
        return Err(GuardError::FeatureMissing {
            pass: pass_name.to_string(),
            warnings,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `ran: true` parses cleanly.
    #[test]
    fn parse_report_returns_true_when_ran_true() {
        let json = r#"{"verb":"enrich_concepts","ran":true,"facts_scanned":42,"attrs_written":10,"edges_written":5,"warnings":[]}"#;
        let (ran, _) = parse_report(json).expect("valid json");
        assert!(ran);
    }

    /// `ran: false` parses cleanly (this is the off-feature path).
    #[test]
    fn parse_report_returns_false_when_ran_false() {
        let json = r#"{"verb":"enrich_metrics","ran":false,"facts_scanned":0,"attrs_written":0,"edges_written":0,"warnings":["enrich_metrics: built without quality-metrics feature"]}"#;
        let (ran, _) = parse_report(json).expect("valid json");
        assert!(!ran);
    }

    /// Unknown extra fields are tolerated (forward-compat with future
    /// EnrichReport extensions).
    #[test]
    fn parse_report_ignores_unknown_fields() {
        let json = r#"{"ran":true,"some_future_field":"hello","another":42}"#;
        let (ran, _) = parse_report(json).expect("valid json");
        assert!(ran);
    }

    /// Missing `ran` field is a parse error (we do not default to true).
    #[test]
    fn parse_report_errors_on_missing_ran_field() {
        let json = r#"{"verb":"enrich_concepts","facts_scanned":42}"#;
        assert!(parse_report(json).is_err());
    }

    /// Malformed JSON is a parse error.
    #[test]
    fn parse_report_errors_on_malformed_json() {
        let json = r#"{"ran": tru"#;
        assert!(parse_report(json).is_err());
    }

    /// `parse_report` carries warnings forward so the FeatureMissing
    /// error can include them in the user-facing message.
    #[test]
    fn parse_report_carries_warnings() {
        let json =
            r#"{"ran":false,"warnings":["enrich_metrics: built without quality-metrics feature"]}"#;
        let (ran, warnings) = parse_report(json).expect("valid json");
        assert!(!ran);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("quality-metrics"));
    }

    /// `parse_report` defaults warnings to empty when absent.
    #[test]
    fn parse_report_defaults_warnings_to_empty() {
        let json = r#"{"ran":true}"#;
        let (ran, warnings) = parse_report(json).expect("valid json");
        assert!(ran);
        assert!(warnings.is_empty());
    }
}

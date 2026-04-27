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

use cfdb_core::enrich::EnrichReport;
use thiserror::Error;

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

/// Parse a `cfdb enrich-<pass>` JSON envelope into a typed
/// [`EnrichReport`]. Wire-form contract guarded by the shared
/// `cfdb-core` struct — `serde` tolerates unknown fields by default,
/// so future additive extensions (new attrs/edges fields) parse cleanly
/// without churning this harness.
pub fn parse_report(json: &str) -> Result<EnrichReport, serde_json::Error> {
    serde_json::from_str(json)
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
    let report = parse_report(&stdout).map_err(|source| GuardError::JsonParse {
        binary: cfdb_bin.display().to_string(),
        stdout: stdout.clone().into_owned(),
        source,
    })?;
    if !report.ran {
        return Err(GuardError::FeatureMissing {
            pass: pass_name.to_string(),
            warnings: report.warnings,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `ran: true` parses cleanly.
    #[test]
    fn parse_report_returns_ran_true() {
        let json = r#"{"verb":"enrich_concepts","ran":true,"facts_scanned":42,"attrs_written":10,"edges_written":5,"warnings":[]}"#;
        assert!(parse_report(json).expect("valid json").ran);
    }

    /// `ran: false` parses cleanly (this is the off-feature path).
    #[test]
    fn parse_report_returns_ran_false() {
        let json = r#"{"verb":"enrich_metrics","ran":false,"facts_scanned":0,"attrs_written":0,"edges_written":0,"warnings":["enrich_metrics: built without quality-metrics feature"]}"#;
        assert!(!parse_report(json).expect("valid json").ran);
    }

    /// Unknown extra fields are tolerated (serde tolerates by default —
    /// forward-compat with future EnrichReport extensions).
    #[test]
    fn parse_report_ignores_unknown_fields() {
        let json = r#"{"verb":"x","ran":true,"facts_scanned":0,"attrs_written":0,"edges_written":0,"warnings":[],"some_future_field":"hello"}"#;
        assert!(parse_report(json).expect("valid json").ran);
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
        let json = r#"{"verb":"enrich_metrics","ran":false,"facts_scanned":0,"attrs_written":0,"edges_written":0,"warnings":["enrich_metrics: built without quality-metrics feature"]}"#;
        let report = parse_report(json).expect("valid json");
        assert!(!report.ran);
        assert_eq!(report.warnings.len(), 1);
        assert!(report.warnings[0].contains("quality-metrics"));
    }
}

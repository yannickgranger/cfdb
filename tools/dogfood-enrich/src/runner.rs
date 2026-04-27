//! Template substitution + tempfile materialization + subprocess
//! invocation. Per RFC-039 §3.5.1.
//!
//! The runner is intentionally split into pure helpers
//! (`substitute_template`) and impure orchestrators (`materialize_and_run`).
//! The pure path is unit-testable; the impure path is exercised by the
//! tests/inject-bite/ fixtures.

use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use thiserror::Error;

/// Substitute the `{{ threshold }}` placeholder in a Cypher template.
///
/// For ratio passes the placeholder is replaced with the threshold
/// value rendered as an integer. For hard-equality passes (`threshold:
/// None`) the template is returned unchanged — those queries do not
/// reference `{{ threshold }}`.
///
/// Pure function. Cypher-comment-aware substitution is deliberately NOT
/// implemented here: the per-pass `.cypher` templates may not put
/// `{{ threshold }}` inside comments, by RFC §3.1 sentinel-pattern note.
pub fn substitute_template(template: &str, threshold: Option<u32>) -> String {
    match threshold {
        Some(value) => template.replace("{{ threshold }}", &value.to_string()),
        None => template.to_string(),
    }
}

/// Errors that bubble up to `EXIT_RUNTIME_ERROR` (1) in the binary.
/// Violation rows are not errors — they return `Ok(RunOutcome::Violations)`
/// which `main.rs` maps to `EXIT_VIOLATIONS` (30).
#[derive(Debug, Error)]
pub enum RunnerError {
    #[error("template file not found: {0}")]
    TemplateMissing(PathBuf),
    #[error("failed to read template {path}: {source}")]
    TemplateRead {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to write materialized template to {path}: {source}")]
    TempfileWrite {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to invoke {binary}: {source}")]
    SubprocessSpawn {
        binary: String,
        #[source]
        source: io::Error,
    },
    #[error("subprocess {binary} terminated by signal (no exit code)")]
    SubprocessSignal { binary: String },
    #[error("subprocess {binary} exited {exit} unexpectedly with --no-fail; stderr: {stderr}")]
    SubprocessUnexpectedExit {
        binary: String,
        exit: i32,
        stderr: String,
    },
    #[error("failed to parse row count from {binary} stdout: {stdout:?}")]
    CountParse { binary: String, stdout: String },
}

/// Outcome of a successful subprocess invocation.
///
/// `Clean` = zero violation rows. `Violations` = ≥1 row found, with
/// the count parsed from `cfdb violations --count-only` stdout. Any
/// runtime error (subprocess fail, unparseable count) bubbles as
/// `RunnerError` rather than being re-classified.
#[derive(Debug, PartialEq, Eq)]
pub enum RunOutcome {
    Clean,
    Violations { row_count: i32 },
}

/// Materialize a Cypher template to a tempfile and invoke
/// `cfdb violations --rule <tempfile> --count-only --no-fail`.
///
/// `--count-only --no-fail` lets the harness read the row count from
/// stdout without `cfdb` itself exiting 30 — we map the count to our
/// own `RunOutcome` here to control the exit-code contract.
pub fn materialize_and_run(
    cfdb_bin: &Path,
    template_path: &Path,
    threshold: Option<u32>,
    db: &Path,
    keyspace: &str,
    tempdir: &Path,
) -> Result<RunOutcome, RunnerError> {
    let raw = std::fs::read_to_string(template_path).map_err(|source| {
        if source.kind() == io::ErrorKind::NotFound {
            RunnerError::TemplateMissing(template_path.to_path_buf())
        } else {
            RunnerError::TemplateRead {
                path: template_path.to_path_buf(),
                source,
            }
        }
    })?;
    let materialized = substitute_template(&raw, threshold);
    let tempfile_path = tempdir.join("self-enrich-materialized.cypher");
    std::fs::write(&tempfile_path, &materialized).map_err(|source| RunnerError::TempfileWrite {
        path: tempfile_path.clone(),
        source,
    })?;
    let output = Command::new(cfdb_bin)
        .arg("violations")
        .arg("--db")
        .arg(db)
        .arg("--keyspace")
        .arg(keyspace)
        .arg("--rule")
        .arg(&tempfile_path)
        .arg("--count-only")
        .arg("--no-fail")
        .output()
        .map_err(|source| RunnerError::SubprocessSpawn {
            binary: cfdb_bin.display().to_string(),
            source,
        })?;
    let exit = output
        .status
        .code()
        .ok_or_else(|| RunnerError::SubprocessSignal {
            binary: cfdb_bin.display().to_string(),
        })?;
    if exit != 0 {
        return Err(RunnerError::SubprocessUnexpectedExit {
            binary: cfdb_bin.display().to_string(),
            exit,
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let count: i32 = stdout.trim().parse().map_err(|_| RunnerError::CountParse {
        binary: cfdb_bin.display().to_string(),
        stdout: stdout.clone().into_owned(),
    })?;
    if count == 0 {
        Ok(RunOutcome::Clean)
    } else {
        Ok(RunOutcome::Violations { row_count: count })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Threshold substitution replaces every occurrence of the
    /// placeholder.
    #[test]
    fn substitute_template_replaces_placeholder() {
        let template = "WHERE nulls * 100 > total * (100 - {{ threshold }})";
        let out = substitute_template(template, Some(95));
        assert_eq!(out, "WHERE nulls * 100 > total * (100 - 95)");
    }

    /// Multiple placeholders in one template are all replaced.
    #[test]
    fn substitute_template_replaces_all_occurrences() {
        let template = "{{ threshold }} or {{ threshold }} again";
        let out = substitute_template(template, Some(80));
        assert_eq!(out, "80 or 80 again");
    }

    /// Hard-equality passes (None threshold) leave the template unchanged.
    #[test]
    fn substitute_template_passthrough_when_threshold_none() {
        let template = "MATCH (i:Item) RETURN i";
        let out = substitute_template(template, None);
        assert_eq!(out, template);
    }

    /// Empty template input is handled correctly.
    #[test]
    fn substitute_template_empty_input() {
        assert_eq!(substitute_template("", Some(95)), "");
        assert_eq!(substitute_template("", None), "");
    }

    /// Template with no placeholder is unchanged when a threshold is
    /// supplied (defensive — a hard-equality query that someone
    /// accidentally invoked with a threshold should not corrupt).
    #[test]
    fn substitute_template_no_placeholder_with_threshold() {
        let template = "MATCH (i:Item) RETURN i";
        assert_eq!(substitute_template(template, Some(95)), template);
    }
}

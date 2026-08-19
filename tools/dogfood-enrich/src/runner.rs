use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use thiserror::Error;

pub fn substitute_template(template: &str, threshold: Option<u32>) -> String {
    match threshold {
        Some(value) => template.replace("{{ threshold }}", &value.to_string()),
        None => template.to_string(),
    }
}

pub fn substitute_named(template: &str, substitutions: &[(&str, &str)]) -> String {
    let mut out = template.to_string();
    for (name, value) in substitutions {
        let placeholder = format!("{{{{ {name} }}}}");
        out = out.replace(&placeholder, value);
    }
    out
}

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

#[derive(Debug, PartialEq, Eq)]
pub enum RunOutcome {
    Clean,
    Violations { row_count: i32 },
}

pub fn materialize_and_run(
    cfdb_bin: &Path,
    template_path: &Path,
    threshold: Option<u32>,
    db: &Path,
    keyspace: &str,
    tempdir: &Path,
) -> Result<RunOutcome, RunnerError> {
    materialize_and_run_with_substitutions(
        cfdb_bin,
        template_path,
        threshold,
        &[],
        db,
        keyspace,
        tempdir,
    )
}

pub fn materialize_and_run_with_substitutions(
    cfdb_bin: &Path,
    template_path: &Path,
    threshold: Option<u32>,
    substitutions: &[(&str, &str)],
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
    let after_threshold = substitute_template(&raw, threshold);
    let materialized = substitute_named(&after_threshold, substitutions);
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

    #[test]
    fn substitute_template_replaces_placeholder() {
        let template = "WHERE nulls * 100 > total * (100 - {{ threshold }})";
        let out = substitute_template(template, Some(95));
        assert_eq!(out, "WHERE nulls * 100 > total * (100 - 95)");
    }

    #[test]
    fn substitute_template_replaces_all_occurrences() {
        let template = "{{ threshold }} or {{ threshold }} again";
        let out = substitute_template(template, Some(80));
        assert_eq!(out, "80 or 80 again");
    }

    #[test]
    fn substitute_template_passthrough_when_threshold_none() {
        let template = "MATCH (i:Item) RETURN i";
        let out = substitute_template(template, None);
        assert_eq!(out, template);
    }

    #[test]
    fn substitute_template_empty_input() {
        assert_eq!(substitute_template("", Some(95)), "");
        assert_eq!(substitute_template("", None), "");
    }

    #[test]
    fn substitute_template_no_placeholder_with_threshold() {
        let template = "MATCH (i:Item) RETURN i";
        assert_eq!(substitute_template(template, Some(95)), template);
    }

    #[test]
    fn substitute_named_replaces_one_placeholder() {
        let template = "WHERE extracted < {{ ground_truth_count }} RETURN extracted";
        let out = substitute_named(template, &[("ground_truth_count", "42")]);
        assert_eq!(out, "WHERE extracted < 42 RETURN extracted");
    }

    #[test]
    fn substitute_named_empty_list_is_passthrough() {
        let template = "MATCH (i:Item) RETURN i";
        assert_eq!(substitute_named(template, &[]), template);
    }

    #[test]
    fn substitute_named_replaces_multiple_distinct_placeholders() {
        let template = "{{ a }} and {{ b }}";
        let out = substitute_named(template, &[("a", "alpha"), ("b", "beta")]);
        assert_eq!(out, "alpha and beta");
    }

    #[test]
    fn substitute_named_leaves_unmapped_placeholder_literal() {
        let template = "{{ a }} and {{ unmapped }}";
        let out = substitute_named(template, &[("a", "alpha")]);
        assert_eq!(out, "alpha and {{ unmapped }}");
    }
}

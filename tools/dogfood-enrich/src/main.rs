//! `dogfood-enrich` binary — RFC-039 §3.5.1 entry point.
//!
//! Subprocess-driven harness. Invokes `cfdb enrich-<pass>` to verify the
//! feature is active (I5.1 guard), then materializes the matching
//! `.cfdb/queries/self-enrich-<pass>.cypher` template with threshold
//! substitution, then invokes `cfdb violations` against the materialized
//! tempfile. Exit codes:
//!
//! - `0`  — zero violation rows (invariant holds).
//! - `30` — at least one violation row (invariant violated, RFC §3.5.1).
//! - `1`  — runtime error: unknown pass, missing template, missing
//!   feature (I5.1), subprocess fail, JSON parse error.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Parser;
use dogfood_enrich::{
    feature_guard, grep_deprecated, passes, runner, EXIT_OK, EXIT_RUNTIME_ERROR, EXIT_VIOLATIONS,
};

#[derive(Debug, Parser)]
#[command(name = "dogfood-enrich", about = "RFC-039 self-enrich dogfood harness")]
struct Cli {
    /// Pass name (one of the 7 RFC-039 passes — see `--list`).
    #[arg(long)]
    pass: String,

    /// Database directory (cfdb keyspace location).
    #[arg(long)]
    db: PathBuf,

    /// Keyspace to extract + dogfood against.
    #[arg(long)]
    keyspace: String,

    /// Path to the `cfdb` binary. Defaults to `target/release/cfdb`.
    #[arg(long, default_value = "target/release/cfdb")]
    cfdb_bin: PathBuf,

    /// Workspace root forwarded to `cfdb enrich-<pass>` when the pass
    /// needs it (rfc-docs, bounded-context, concepts, git-history).
    #[arg(long)]
    workspace: Option<PathBuf>,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(code) => ExitCode::from(code as u8),
        Err(message) => {
            eprintln!("dogfood-enrich: {message}");
            ExitCode::from(EXIT_RUNTIME_ERROR as u8)
        }
    }
}

fn run(cli: Cli) -> Result<i32, String> {
    let pass = passes::PassDef::by_name(&cli.pass).ok_or_else(|| {
        let names: Vec<&str> = passes::PassDef::all().iter().map(|p| p.name).collect();
        format!("unknown pass {:?}. Valid: {}", cli.pass, names.join(", "))
    })?;

    // I5.1 feature-presence guard.
    feature_guard::check_pass_ran(
        &cli.cfdb_bin,
        pass.name,
        &cli.db,
        &cli.keyspace,
        cli.workspace.as_deref(),
    )
    .map_err(|e| format!("{e}"))?;

    // Materialize template + run violations.
    let tempdir = tempfile::tempdir().map_err(|e| format!("failed to create tempdir: {e}"))?;
    let template_path = PathBuf::from(pass.query_template_path);
    let extra_owned = compute_extra_substitutions(pass.name, cli.workspace.as_deref())?;
    let extra_borrows: Vec<(&str, &str)> = extra_owned
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    let outcome = runner::materialize_and_run_with_substitutions(
        &cli.cfdb_bin,
        &template_path,
        pass.threshold,
        &extra_borrows,
        &cli.db,
        &cli.keyspace,
        tempdir.path(),
    )
    .map_err(|e| format!("{e}"))?;

    let short = pass.name.strip_prefix("enrich-").unwrap_or(pass.name);
    match outcome {
        runner::RunOutcome::Clean => {
            println!("self-enrich-{short}: 0 violations (invariant holds)");
            Ok(EXIT_OK)
        }
        runner::RunOutcome::Violations { row_count } => {
            eprintln!("self-enrich-{short}: {row_count} violation row(s) — invariant FAILED");
            Ok(EXIT_VIOLATIONS)
        }
    }
}

/// Compute per-pass extra placeholders that the runner must substitute
/// before submitting the materialized template to `cfdb violations`.
///
/// Most passes return an empty map (their templates use only the
/// `{{ threshold }}` placeholder). `enrich-deprecation` is the
/// exception: its sentinel compares the extracted-graph count against
/// a source-side ground truth, computed by walking `--workspace` and
/// counting `#[deprecated]` attribute occurrences.
///
/// Errors propagate to `EXIT_RUNTIME_ERROR` (1) — a missing workspace
/// or unreadable source file is a configuration problem, not a
/// violation.
fn compute_extra_substitutions(
    pass_name: &str,
    workspace: Option<&Path>,
) -> Result<Vec<(String, String)>, String> {
    if pass_name != "enrich-deprecation" {
        return Ok(Vec::new());
    }
    let root = workspace.ok_or_else(|| {
        "enrich-deprecation requires --workspace to compute the source-side \
         #[deprecated] ground truth"
            .to_string()
    })?;
    let count = grep_deprecated::count_deprecated_in_workspace(root)
        .map_err(|e| format!("failed to grep #[deprecated] under {}: {e}", root.display()))?;
    Ok(vec![("ground_truth_count".to_string(), count.to_string())])
}

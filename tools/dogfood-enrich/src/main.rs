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
    count_items, feature_guard, grep_deprecated, grep_rfc_docs, passes, runner, scan_concepts,
    thresholds, EXIT_OK, EXIT_RUNTIME_ERROR, EXIT_VIOLATIONS,
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
        pass.cli_takes_workspace,
    )
    .map_err(|e| format!("{e}"))?;

    // Materialize template + run violations.
    let tempdir = tempfile::tempdir().map_err(|e| format!("failed to create tempdir: {e}"))?;
    let template_path = PathBuf::from(pass.query_template_path);
    let extra_owned = compute_extra_substitutions(
        pass.name,
        cli.workspace.as_deref(),
        &cli.cfdb_bin,
        &cli.db,
        &cli.keyspace,
    )?;
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
    cfdb_bin: &Path,
    db: &Path,
    keyspace: &str,
) -> Result<Vec<(String, String)>, String> {
    match pass_name {
        "enrich-deprecation" => {
            let root = workspace.ok_or_else(|| {
                "enrich-deprecation requires --workspace to compute the source-side \
                 #[deprecated] ground truth"
                    .to_string()
            })?;
            let count = grep_deprecated::count_deprecated_in_workspace(root).map_err(|e| {
                format!("failed to grep #[deprecated] under {}: {e}", root.display())
            })?;
            Ok(vec![("ground_truth_count".to_string(), count.to_string())])
        }
        "enrich-rfc-docs" => {
            let root = workspace.ok_or_else(|| {
                "enrich-rfc-docs requires --workspace to count docs/RFC-*.md files".to_string()
            })?;
            let count = grep_rfc_docs::count_rfc_md_files(root).map_err(|e| {
                format!(
                    "failed to count docs/RFC-*.md under {}: {e}",
                    root.display()
                )
            })?;
            Ok(vec![("ground_truth_count".to_string(), count.to_string())])
        }
        "enrich-concepts" => {
            let root = workspace.ok_or_else(|| {
                "enrich-concepts requires --workspace to scan .cfdb/concepts/*.toml".to_string()
            })?;
            let counts = scan_concepts::scan_concepts(root).map_err(|e| {
                format!(
                    "failed to scan .cfdb/concepts/*.toml under {}: {e}",
                    root.display()
                )
            })?;
            Ok(vec![
                (
                    "declared_context_count".to_string(),
                    counts.distinct_context_names.to_string(),
                ),
                (
                    "declared_canonical_crate_count".to_string(),
                    counts.declared_canonical_crate_count.to_string(),
                ),
            ])
        }
        // Path B from #355 — keyspace-side ratio computation. The
        // cfdb-query v0.1 subset has no arithmetic operators, so the
        // ratio `nulls/total < threshold/100` is computed harness-side
        // and substituted into the template as a flat absolute count.
        "enrich-bounded-context" => {
            ratio_substitutions(
                cfdb_bin,
                db,
                keyspace,
                None, // every :Item, no kind filter
                thresholds::BC_COVERAGE_THRESHOLD,
                "BC_COVERAGE_THRESHOLD",
            )
        }
        // Same Path B shape — denominator is `:Item{kind:"fn"}` per
        // RFC-039 §3.1 reachability + metrics rows. The kind filter is
        // applied harness-side via `count_items_with_kind` so the
        // sentinel template can read the matching `total_items`.
        "enrich-reachability" => ratio_substitutions(
            cfdb_bin,
            db,
            keyspace,
            Some("fn"),
            thresholds::REACHABILITY_THRESHOLD,
            "REACHABILITY_THRESHOLD",
        ),
        "enrich-metrics" => ratio_substitutions(
            cfdb_bin,
            db,
            keyspace,
            Some("fn"),
            thresholds::METRICS_COVERAGE_THRESHOLD,
            "METRICS_COVERAGE_THRESHOLD",
        ),
        // Git-history denominator is every `:Item` (per RFC-039 §3.1
        // git-history row) — same shape as bounded-context.
        "enrich-git-history" => ratio_substitutions(
            cfdb_bin,
            db,
            keyspace,
            None,
            thresholds::GIT_COVERAGE_THRESHOLD,
            "GIT_COVERAGE_THRESHOLD",
        ),
        _ => Ok(Vec::new()),
    }
}

/// Shared Path B (#355) helper for ratio passes — count `:Item` (or
/// kind-filtered) in the keyspace, derive `nulls_threshold` from the
/// passed threshold const, return both as substitutions for the
/// `{{ total_items }}` and `{{ nulls_threshold }}` placeholders.
///
/// `kind` is `Some("fn")` for `enrich-reachability` + `enrich-metrics`
/// (denominators are functions only) and `None` for the kind-agnostic
/// passes (`enrich-bounded-context`, `enrich-git-history`).
///
/// `threshold` is the pre-validated `Option<u32>` from
/// `thresholds::*_THRESHOLD`; the `name` parameter is just the const's
/// identifier as a `&str` so the error message points the reader at the
/// right line in `thresholds.rs` if a const accidentally drifts to None.
fn ratio_substitutions(
    cfdb_bin: &Path,
    db: &Path,
    keyspace: &str,
    kind: Option<&str>,
    threshold: Option<u32>,
    threshold_name: &str,
) -> Result<Vec<(String, String)>, String> {
    let total = count_items::count_items_with_kind(cfdb_bin, db, keyspace, kind).map_err(|e| {
        format!(
            "failed to count :Item{}: {e}",
            kind.map(|k| format!(" with kind={k}")).unwrap_or_default()
        )
    })?;
    let threshold_pct = threshold.ok_or_else(|| {
        format!("{threshold_name} must be Some — this is a ratio pass; check tools/dogfood-enrich/src/thresholds.rs")
    })?;
    // Integer floor — at small fixture scale the threshold floors to 0,
    // so any single null fires the sentinel (#345 AC-3 contract; same
    // math applies to all four ratio passes).
    let nulls_threshold =
        total.saturating_mul(100usize.saturating_sub(threshold_pct as usize)) / 100;
    Ok(vec![
        ("total_items".to_string(), total.to_string()),
        ("nulls_threshold".to_string(), nulls_threshold.to_string()),
    ])
}

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Parser;
use dogfood_enrich::{
    count_items, extracted_files, feature_guard, grep_deprecated, grep_rfc_docs, passes, runner,
    scan_concepts, thresholds, EXIT_OK, EXIT_RUNTIME_ERROR, EXIT_VIOLATIONS,
};

#[derive(Debug, Parser)]
#[command(name = "dogfood-enrich", about = "RFC-039 self-enrich dogfood harness")]
struct Cli {
    #[arg(help = "Pass name (one of the 7 RFC-039 passes — see `--list`)")]
    #[arg(long)]
    pass: String,

    #[arg(help = "Database directory (cfdb keyspace location)")]
    #[arg(long)]
    db: PathBuf,

    #[arg(help = "Keyspace to extract + dogfood against")]
    #[arg(long)]
    keyspace: String,

    #[arg(help = "Path to the `cfdb` binary. Defaults to `target/release/cfdb`")]
    #[arg(long, default_value = "target/release/cfdb")]
    cfdb_bin: PathBuf,

    #[arg(
        help = "Workspace root forwarded to `cfdb enrich-<pass>` when the pass needs it (rfc-docs, bounded-context, concepts, git-history)"
    )]
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

    feature_guard::check_pass_ran(
        &cli.cfdb_bin,
        pass.name,
        &cli.db,
        &cli.keyspace,
        cli.workspace.as_deref(),
        pass.cli_takes_workspace,
    )
    .map_err(|e| format!("{e}"))?;

    let tempdir = tempfile::tempdir().map_err(|e| format!("failed to create tempdir: {e}"))?;
    let template_path = PathBuf::from(pass.query_template_path);
    let extra_owned = compute_extra_substitutions(
        pass.name,
        cli.workspace.as_deref(),
        &cli.cfdb_bin,
        &cli.db,
        &cli.keyspace,
    )?;
    if pass.name == "enrich-deprecation" {
        if let Some((_, truth)) = extra_owned.iter().find(|(k, _)| k == "ground_truth_count") {
            let truth: usize = truth
                .parse()
                .map_err(|e| format!("internal: ground_truth_count {truth:?} not a usize: {e}"))?;
            if truth >= 1 {
                let extracted = count_items::count_deprecated_items_in_keyspace(
                    &cli.cfdb_bin,
                    &cli.db,
                    &cli.keyspace,
                )
                .map_err(|e| format!("{e}"))?;
                if extracted == 0 {
                    eprintln!(
                        "self-enrich-deprecation: keyspace has 0 :Item.is_deprecated but the \
                         source-side ground truth is {truth} — invariant FAILED (zero-extracted \
                         guard; the count() sentinel cannot fire on an empty MATCH, see #564)"
                    );
                    return Ok(EXIT_VIOLATIONS);
                }
            }
        }
    }

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
            let files = extracted_files::file_paths_in_keyspace(cfdb_bin, db, keyspace)
                .map_err(|e| format!("failed to read :File set from keyspace: {e}"))?;
            let count = grep_deprecated::count_deprecated_in_files(root, &files).map_err(|e| {
                format!(
                    "failed to count #[deprecated] under {}: {e}",
                    root.display()
                )
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
        "enrich-bounded-context" => ratio_substitutions(
            cfdb_bin,
            db,
            keyspace,
            None,
            thresholds::BC_COVERAGE_THRESHOLD,
            "BC_COVERAGE_THRESHOLD",
        ),
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
    let nulls_threshold =
        total.saturating_mul(100usize.saturating_sub(threshold_pct as usize)) / 100;
    Ok(vec![
        ("total_items".to_string(), total.to_string()),
        ("nulls_threshold".to_string(), nulls_threshold.to_string()),
    ])
}

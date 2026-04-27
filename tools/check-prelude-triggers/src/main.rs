//! `check-prelude-triggers` — Tier-1 binary entry point per RFC-034 v3.3 §4.2.
//!
//! Each subcommand runs exactly one C-trigger. The binary is stateless: it
//! reads argv-supplied TOML + diff files, emits a versioned JSON envelope on
//! stdout, and exits with a code per RFC-034 §4.2 rust-systems Amendment 1:
//!
//! - `0` success (envelope always emitted — empty `triggers_fired` is valid)
//! - `1` usage / argument error (clap parse failure)
//! - `2` fatal runtime error (TOML parse, IO)

use std::path::PathBuf;
use std::process::ExitCode;

use check_prelude_triggers::{
    report::PreludeTriggerReport,
    trigger_id::TriggerId,
    triggers::{
        c1_cross_context, c3_port_signature, c7_financial_precision, c8_pipeline_stage,
        c9_workspace_cardinality, TriggerOutcome,
    },
    validate_freshness, LoadError,
};
use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "check-prelude-triggers",
    about = "RFC-034 v3.3 Tier-1 C-trigger binary — mechanical pre-council gates",
    version
)]
struct Cli {
    /// Git base ref of the diff under inspection (e.g. `develop`).
    #[arg(long, global = true, default_value = "")]
    from_ref: String,
    /// Git head ref of the diff under inspection (e.g. the PR HEAD SHA).
    #[arg(long, global = true, default_value = "")]
    to_ref: String,
    /// Envelope schema version the consumer expects. Only `v1` is recognized
    /// today; any other value fails fast.
    #[arg(long, global = true, default_value = "v1")]
    schema_version: String,
    /// Refuse to emit an envelope when `--from-ref` equals `--to-ref`. Used by
    /// `/ship` pre-flight to ensure the capture reflects a real diff and not
    /// an issue-start snapshot. Default off — issue-start captures remain
    /// valid for archaeology / dogfood replay use cases.
    #[arg(long, global = true)]
    require_fresh: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// C1 — cross-context change. Fires when ≥2 bounded contexts from
    /// `context-map.toml` are touched by the diff.
    C1CrossContext {
        #[arg(long)]
        context_map: PathBuf,
        #[arg(long)]
        changed_paths: PathBuf,
    },
    /// C3 — port trait signature. Fires when any changed path matches
    /// `^crates/ports[^/]*/src/`.
    C3PortSignature {
        #[arg(long)]
        changed_paths: PathBuf,
    },
    /// C7 — financial-precision path. Fires when any changed path is under
    /// a prefix declared in `financial-precision-crates.toml`.
    C7FinancialPrecision {
        #[arg(long)]
        financial_precision_crates: PathBuf,
        #[arg(long)]
        changed_paths: PathBuf,
    },
    /// C8 — pipeline-stage cross. Fires when the diff touches ≥2 stages in
    /// `pipeline-stages.toml`.
    C8PipelineStage {
        #[arg(long)]
        pipeline_stages: PathBuf,
        #[arg(long)]
        changed_paths: PathBuf,
    },
    /// C9 — workspace cardinality. Fires when the workspace root `Cargo.toml`
    /// is in the diff; parses `[workspace] members = [...]` directly (no
    /// `cargo metadata` subprocess).
    C9WorkspaceCardinality {
        #[arg(long)]
        workspace_root: PathBuf,
        #[arg(long)]
        changed_paths: PathBuf,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    if cli.schema_version != "v1" {
        eprintln!(
            "error: unknown --schema-version {:?}; only \"v1\" is recognized",
            cli.schema_version
        );
        return ExitCode::from(1);
    }

    if let Err(msg) = validate_freshness(cli.require_fresh, &cli.from_ref, &cli.to_ref) {
        eprintln!("error: {msg}");
        return ExitCode::from(1);
    }

    let (id, outcome) = match run_command(&cli.command) {
        Ok(pair) => pair,
        Err(err) => {
            eprintln!("error: {err}");
            return ExitCode::from(2);
        }
    };

    let mut report = PreludeTriggerReport::new(cli.from_ref, cli.to_ref);
    if outcome.fired {
        report.record(id, outcome.evidence);
    } else {
        // Record un-fired evidence under the trigger's ID so consumers can
        // inspect what was checked even when nothing fired. The report still
        // has `triggers_fired = []` to signal "no pre-council required".
        report
            .evidence
            .insert(id.as_str().to_string(), outcome.evidence);
    }

    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    if serde_json::to_writer(&mut handle, &report).is_err() {
        return ExitCode::from(2);
    }
    if std::io::Write::write_all(&mut handle, b"\n").is_err() {
        return ExitCode::from(2);
    }

    ExitCode::SUCCESS
}

fn run_command(cmd: &Command) -> Result<(TriggerId, TriggerOutcome), LoadError> {
    match cmd {
        Command::C1CrossContext {
            context_map,
            changed_paths,
        } => Ok((
            TriggerId::C1,
            c1_cross_context::run(context_map, changed_paths)?,
        )),
        Command::C3PortSignature { changed_paths } => {
            Ok((TriggerId::C3, c3_port_signature::run(changed_paths)?))
        }
        Command::C7FinancialPrecision {
            financial_precision_crates,
            changed_paths,
        } => Ok((
            TriggerId::C7,
            c7_financial_precision::run(financial_precision_crates, changed_paths)?,
        )),
        Command::C8PipelineStage {
            pipeline_stages,
            changed_paths,
        } => Ok((
            TriggerId::C8,
            c8_pipeline_stage::run(pipeline_stages, changed_paths)?,
        )),
        Command::C9WorkspaceCardinality {
            workspace_root,
            changed_paths,
        } => Ok((
            TriggerId::C9,
            c9_workspace_cardinality::run(workspace_root, changed_paths)?,
        )),
    }
}

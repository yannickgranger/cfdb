//! `cfdb` — CLI wire form.
//!
//! Exposes the full cfdb API surface as clap subcommands:
//!
//! INGEST (8):
//! - `cfdb extract --workspace <path> --db <path> [--keyspace <name>]`
//! - `cfdb enrich-git-history --db <path> --keyspace <name>`      (Phase A stub)
//! - `cfdb enrich-rfc-docs --db <path> --keyspace <name>`         (Phase A stub)
//! - `cfdb enrich-deprecation --db <path> --keyspace <name>`      (Phase A stub)
//! - `cfdb enrich-bounded-context --db <path> --keyspace <name>`  (Phase A stub)
//! - `cfdb enrich-concepts --db <path> --keyspace <name>`         (Phase A stub)
//! - `cfdb enrich-reachability --db <path> --keyspace <name>`     (Phase A stub)
//! - `cfdb enrich-metrics --db <path> --keyspace <name>`          (Phase A stub — deferred)
//!
//! RAW (1):
//! - `cfdb query --db <path> --keyspace <name> <cypher> [--params <json>] [--input <yaml>]`
//!
//! TYPED (8):
//! - `cfdb find-canonical --db <path> --keyspace <name> --concept <c>` (Phase A stub)
//! - `cfdb list-callers --db <path> --keyspace <name> --qname <regex>` (wired)
//! - `cfdb violations --db <path> --keyspace <name> --rule <file.cypher>`
//! - `cfdb check --db <path> --keyspace <name> --trigger <T1|T3> [--no-fail]` (editorial-drift triggers)
//! - `cfdb check-predicate --db <path> --keyspace <name> --workspace-root <path> --name <predicate> [--param <name>:<form>:<value> ...] [--format text|json] [--no-fail]` (named-predicate library)
//! - `cfdb list-bypasses --db <path> --keyspace <name> --concept <c>`  (Phase A stub)
//! - `cfdb list-items-matching --db <path> --keyspace <name> --name-pattern <r> [--kinds <list>] [--group-by-context]`
//! - `cfdb scope --db <path> --context <name> [--workspace <path>] [--format json|table] [--output <path>] [--keyspace <name>]`
//!
//! SNAPSHOT (3):
//! - `cfdb snapshots --db <path>`
//! - `cfdb diff --db <path> --a <ks_a> --b <ks_b> [--kinds <list>]`    (Phase A stub)
//! - `cfdb drop --db <path> --keyspace <name>`
//!
//! SCHEMA (2 — version covered by `cfdb version`):
//! - `cfdb version`                                                — schema_version
//! - `cfdb schema-describe`                                        — full schema JSON
//!
//! AUX (existing helpers):
//! - `cfdb dump --db <path> --keyspace <name>`               — canonical sorted dump
//! - `cfdb export --db <path> --keyspace <name> [--format sorted-jsonl]` — alias of `dump`
//! - `cfdb list-keyspaces --db <path>`                       — convenience listing
//!
//! Exit codes:
//! - `0` — success (no findings, or `--no-fail` set)
//! - `1` — runtime error (extractor panic, IO failure, parse error in rule,
//!   any handler returns `Err`)
//! - `2` — usage error (clap parse failure: unknown flag, missing required
//!   arg, unknown enum value)
//! - `30` — findings present, gate failure (rule rows returned by
//!   `cfdb violations` / `cfdb check` / `cfdb check-predicate` without
//!   `--no-fail`). Mirrors a convention so CI scripts can disambiguate
//!   "extractor blew up" (1) from "rule found rows" (30).
//!
//! NOTE — downstream consumers reading exit code 1 as "findings" must
//! update to read 30.
//!
//! The `--db` path is a directory containing one `{keyspace}.json` file per
//! keyspace. Extract writes; query/dump/list read.

mod main_command;
mod main_dispatch;
mod main_exit;
mod main_parse;

use std::process::ExitCode;

use cfdb_cli::{schema_describe_cmd, CfdbCliError};
use clap::Parser;

use crate::main_command::Command;
use crate::main_dispatch::{dispatch_core, dispatch_enrich, dispatch_snapshot, dispatch_typed};
use crate::main_exit::exit_code_for;

#[derive(Debug, Parser)]
#[command(name = "cfdb", version, about = "code facts database")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("cfdb: {e}");
            // `exit_code_for` maps Usage → 2, all other variants → 1.
            // The findings path (exit 30) is handled in `main_dispatch` via
            // `findings_exit()` and never reaches here.
            ExitCode::from(exit_code_for(&e) as u8)
        }
    }
}

fn run(cli: Cli) -> Result<(), CfdbCliError> {
    match cli.command {
        Command::Version => print_version(),
        Command::SchemaDescribe => schema_describe_cmd()?,
        cmd @ (Command::Extract(_)
        | Command::Query(..)
        | Command::Violations { .. }
        | Command::Check { .. }
        | Command::Dump { .. }
        | Command::Export { .. }
        | Command::ListKeyspaces { .. }) => dispatch_core(cmd)?,
        cmd @ (Command::EnrichGitHistory { .. }
        | Command::EnrichRfcDocs { .. }
        | Command::EnrichDeprecation { .. }
        | Command::EnrichBoundedContext { .. }
        | Command::EnrichConcepts { .. }
        | Command::EnrichReachability { .. }
        | Command::EnrichMetrics { .. }) => dispatch_enrich(cmd)?,
        cmd @ (Command::FindCanonical { .. }
        | Command::ListCallers { .. }
        | Command::Impact(..)
        | Command::ListBypasses { .. }
        | Command::ListItemsMatching { .. }
        | Command::Scope { .. }
        | Command::CheckPredicate { .. }) => dispatch_typed(cmd)?,
        cmd @ (Command::Snapshots { .. }
        | Command::Diff { .. }
        | Command::Classify { .. }
        | Command::Drop { .. }) => dispatch_snapshot(cmd)?,
    }
    Ok(())
}

fn print_version() {
    println!("cfdb {}", env!("CARGO_PKG_VERSION"));
    println!("schema {}", cfdb_core::SchemaVersion::CURRENT);
}

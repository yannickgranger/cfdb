//! `cfdb` — CLI wire form for cfdb v0.1 + v0.2 (RFC §6.2 / §11).
//!
//! Exposes the full 20-verb cfdb API surface (RFC §6 + council-cfdb-wiring
//! RATIFIED §A.14 + §A.17 + #43 council round 1 §43-A) as clap subcommands:
//!
//! INGEST (8 — post #43-A amendment):
//! - `cfdb extract --workspace <path> --db <path> [--keyspace <name>]`
//! - `cfdb enrich-git-history --db <path> --keyspace <name>`      (Phase A stub — slice 43-B)
//! - `cfdb enrich-rfc-docs --db <path> --keyspace <name>`         (Phase A stub — slice 43-D)
//! - `cfdb enrich-deprecation --db <path> --keyspace <name>`      (Phase A stub — slice 43-C)
//! - `cfdb enrich-bounded-context --db <path> --keyspace <name>`  (Phase A stub — slice 43-E)
//! - `cfdb enrich-concepts --db <path> --keyspace <name>`         (Phase A stub — slice 43-F)
//! - `cfdb enrich-reachability --db <path> --keyspace <name>`     (Phase A stub — slice 43-G)
//! - `cfdb enrich-metrics --db <path> --keyspace <name>`          (Phase A stub — deferred, out of #43 scope)
//!
//! RAW (1):
//! - `cfdb query --db <path> --keyspace <name> <cypher> [--params <json>] [--input <yaml>]`
//!
//! TYPED (6):
//! - `cfdb find-canonical --db <path> --keyspace <name> --concept <c>` (Phase A stub)
//! - `cfdb list-callers --db <path> --keyspace <name> --qname <regex>` (wired — #3633)
//! - `cfdb violations --db <path> --keyspace <name> --rule <file.cypher>`
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
//! AUX (existing helpers, RFC §11 wire-form ergonomics):
//! - `cfdb dump --db <path> --keyspace <name>`               — canonical sorted dump
//! - `cfdb export --db <path> --keyspace <name> [--format sorted-jsonl]` — alias of `dump`
//! - `cfdb list-keyspaces --db <path>`                       — convenience listing
//!
//! Exit codes:
//! - `0` — success
//! - `1` — runtime error (any handler returns `Err`)
//! - `2` — usage error (clap parse failure)
//!
//! The `--db` path is a directory containing one `{keyspace}.json` file per
//! keyspace. Extract writes; query/dump/list read.

mod main_command;
mod main_dispatch;
mod main_parse;

use std::process::ExitCode;

use cfdb_cli::{schema_describe_cmd, CfdbCliError};
use clap::Parser;

use crate::main_command::Command;
use crate::main_dispatch::{dispatch_core, dispatch_enrich, dispatch_snapshot, dispatch_typed};

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
            ExitCode::from(1)
        }
    }
}

fn run(cli: Cli) -> Result<(), CfdbCliError> {
    match cli.command {
        Command::Version => print_version(),
        Command::SchemaDescribe => schema_describe_cmd()?,
        cmd @ (Command::Extract { .. }
        | Command::Query { .. }
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
        | Command::ListBypasses { .. }
        | Command::ListItemsMatching { .. }
        | Command::Scope { .. }) => dispatch_typed(cmd)?,
        cmd @ (Command::Snapshots { .. } | Command::Diff { .. } | Command::Drop { .. }) => {
            dispatch_snapshot(cmd)?
        }
    }
    Ok(())
}

fn print_version() {
    println!("cfdb {}", env!("CARGO_PKG_VERSION"));
    println!("schema {}", cfdb_core::SchemaVersion::CURRENT);
}

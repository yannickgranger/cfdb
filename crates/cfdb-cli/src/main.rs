mod main_command;
mod main_dispatch;
mod main_exit;
mod main_parse;

use std::process::ExitCode;

use cfdb_cli::{schema_describe_cmd, CfdbCliError};
use clap::Parser;

use crate::main_command::Command;
#[cfg(feature = "classify")]
use crate::main_dispatch::dispatch_classify;
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
        cmd @ (Command::ListCallers { .. }
        | Command::Impact(..)
        | Command::ListItemsMatching { .. }
        | Command::CheckPredicate { .. }) => dispatch_typed(cmd)?,
        cmd @ (Command::Snapshots { .. } | Command::Diff { .. } | Command::Drop { .. }) => {
            dispatch_snapshot(cmd)?
        }
        #[cfg(feature = "classify")]
        cmd @ (Command::Scope { .. }
        | Command::Classify { .. }
        | Command::Check { .. }
        | Command::FindCanonical { .. }
        | Command::ListBypasses { .. }) => dispatch_classify(cmd)?,
    }
    Ok(())
}

fn print_version() {
    println!("cfdb {}", env!("CARGO_PKG_VERSION"));
    println!("schema {}", cfdb_core::SchemaVersion::CURRENT);
}

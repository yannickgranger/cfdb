//! `cfdb query` argument struct. Lifted out of the parent `args.rs` to keep it
//! under the 500-LoC god-file threshold — the same remedy `extract_args.rs` /
//! `impact_args.rs` applied. `Command::Query(QueryArgs)` flattens via
//! `#[derive(clap::Args)]`, so the CLI UX (`cfdb query <CYPHER> --db ...`) is
//! unchanged; only the internal Rust data shape moves.

use std::path::PathBuf;

use clap::Args;

/// Run a Cypher-subset query against a loaded keyspace.
#[derive(Debug, Args)]
pub(crate) struct QueryArgs {
    /// Directory containing per-keyspace JSON files.
    #[arg(long)]
    pub db: PathBuf,
    /// Keyspace to query.
    #[arg(long)]
    pub keyspace: String,
    /// The Cypher-subset query source.
    pub cypher: String,
    /// Inline JSON object of parameter substitutions, e.g.
    /// `--params '{"crate":"cfdb-core"}'`. Phase A: parsed but not yet
    /// threaded through the evaluator (RFC §6.2 — wire form first).
    #[arg(long)]
    pub params: Option<String>,
    /// Path to a YAML file providing the `sets?` external buckets used
    /// by `query_with_input` patterns (e.g. raid plans). Phase A:
    /// accepted but not yet wired (RFC §6.2 — wire form first).
    #[arg(long)]
    pub input: Option<PathBuf>,
}

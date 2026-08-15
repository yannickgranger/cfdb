//! `cfdb query` argument struct. `Command::Query(QueryArgs)` flattens via
//! `#[derive(clap::Args)]`, so the CLI UX (`cfdb query <CYPHER> --db ...`) is
//! unchanged.

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
    /// `--params '{"crate":"cfdb-core"}'`. Parsed but not yet threaded
    /// through the evaluator.
    #[arg(long)]
    pub params: Option<String>,
    /// Path to a YAML file providing the `sets?` external buckets used
    /// by `query_with_input` patterns (e.g. raid plans). Accepted but not
    /// yet wired.
    #[arg(long)]
    pub input: Option<PathBuf>,
}

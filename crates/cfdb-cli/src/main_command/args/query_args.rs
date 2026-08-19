use std::path::PathBuf;

use clap::Args;

#[derive(Debug, Args)]
#[command(about = "Run a Cypher-subset query against a loaded keyspace")]
pub(crate) struct QueryArgs {
    #[arg(help = "Directory containing per-keyspace JSON files")]
    #[arg(long)]
    pub db: PathBuf,
    #[arg(help = "Keyspace to query")]
    #[arg(long)]
    pub keyspace: String,
    #[arg(help = "The Cypher-subset query source")]
    pub cypher: String,
    #[arg(
        help = "Inline JSON object of parameter substitutions, e.g. `--params '{\"crate\":\"cfdb-core\"}'`. Parsed but not yet threaded through the evaluator"
    )]
    #[arg(long)]
    pub params: Option<String>,
    #[arg(
        help = "Path to a YAML file providing the `sets?` external buckets used by `query_with_input` patterns (e.g. raid plans). Accepted but not yet wired"
    )]
    #[arg(long)]
    pub input: Option<PathBuf>,
}

use std::path::PathBuf;

use clap::Args;

#[derive(Debug, Args)]
#[command(
    about = "Blast radius — every transitive caller of the changed items. Seeds come from `--item <qname>` (repeatable, exact) OR `--since <ref>` (the items defined in the files `git diff --name-only <ref>..HEAD` reports). Composes the canonical `impact_query`"
)]
pub(crate) struct ImpactArgs {
    #[arg(help = "Directory containing per-keyspace JSON files")]
    #[arg(long)]
    pub db: PathBuf,
    #[arg(help = "Keyspace to query")]
    #[arg(long)]
    pub keyspace: String,
    #[arg(help = "Seed qname (repeatable). Provide this or `--since`")]
    #[arg(long)]
    pub item: Vec<String>,
    #[arg(help = "Git ref — seeds are the items in the files changed since it")]
    #[arg(long)]
    pub since: Option<String>,
    #[arg(help = "Workspace root for the `--since` `git diff`. Defaults to `.`")]
    #[arg(long, default_value = ".")]
    pub workspace: PathBuf,
    #[arg(
        help = "Bound the reverse traversal to N hops (`CALLS*1..N`). Omitted = the open, unbounded form (every transitive caller)"
    )]
    #[arg(long)]
    pub max_depth: Option<u32>,
}

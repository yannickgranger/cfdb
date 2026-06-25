//! `cfdb impact` argument struct. Lifted out of the parent `args.rs` to keep
//! it under the 500-LoC god-file threshold — the same remedy `extract_args.rs`
//! applied. `Command::Impact(ImpactArgs)` flattens via `#[derive(clap::Args)]`,
//! so the CLI UX (`cfdb impact --item ... --since ...`) is unchanged; only the
//! internal Rust data shape moves.

use std::path::PathBuf;

use clap::Args;

/// Blast radius — every transitive caller of the changed items. Seeds come
/// from `--item <qname>` (repeatable, exact) OR `--since <ref>` (the items
/// defined in the files `git diff --name-only <ref>..HEAD` reports). Composes
/// the canonical `impact_query`. RFC-047 §3.3 / slice 47-B.
#[derive(Debug, Args)]
pub(crate) struct ImpactArgs {
    /// Directory containing per-keyspace JSON files.
    #[arg(long)]
    pub db: PathBuf,
    /// Keyspace to query.
    #[arg(long)]
    pub keyspace: String,
    /// Seed qname (repeatable). Provide this or `--since`.
    #[arg(long)]
    pub item: Vec<String>,
    /// Git ref — seeds are the items in the files changed since it.
    #[arg(long)]
    pub since: Option<String>,
    /// Workspace root for the `--since` `git diff`. Defaults to `.`.
    #[arg(long, default_value = ".")]
    pub workspace: PathBuf,
    /// Bound the reverse traversal to N hops (`CALLS*1..N`). Omitted = the open,
    /// unbounded form (every transitive caller). RFC-047a §6.
    #[arg(long)]
    pub max_depth: Option<u32>,
}

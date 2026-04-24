//! `cfdb extract` argument struct. Lifted out of the parent `args.rs`
//! in a second #248 slice because `args.rs` alone crossed the 500-LoC
//! god-file threshold after agent C's initial move. Subcommand-enum
//! flattening via `#[derive(clap::Args)]` preserves the exact CLI UX
//! (`cfdb extract --workspace ... --db ...`) — only the internal Rust
//! data shape changes: `Command::Extract(ExtractArgs)` instead of
//! `Command::Extract { workspace, db, ... }`.

use std::path::PathBuf;

use clap::Args;

/// Extract facts from a Rust workspace into a keyspace on disk.
#[derive(Debug, Args)]
pub(crate) struct ExtractArgs {
    /// Root of the target Rust workspace (must contain Cargo.toml).
    /// When `--rev` is passed, this is the git repository root and
    /// extraction walks a temporary worktree checked out at `<rev>`
    /// rather than the live tree.
    #[arg(long)]
    pub workspace: PathBuf,
    /// Directory to write the per-keyspace JSON files into.
    #[arg(long)]
    pub db: PathBuf,
    /// Keyspace name. Defaults to the basename of `--workspace`
    /// (or the short `<rev>` when `--rev` is passed without an
    /// explicit keyspace).
    #[arg(long)]
    pub keyspace: Option<String>,
    /// Run the HIR-based extractor after syn to add resolved
    /// `:CallSite`, `CALLS`, `INVOKES_AT`, `:EntryPoint`, and
    /// `EXPOSES` facts. Requires the `hir` Cargo feature —
    /// rebuild with `cargo build -p cfdb-cli --features hir`
    /// to opt in (Issue #86 / slice 4).
    #[arg(long)]
    pub hir: bool,
    /// Extract against a specific git revision. Accepts two forms:
    ///
    ///   1. `<sha|tag|branch>` — same-repo: requires `--workspace`
    ///      to point at a git repository root; shells out to
    ///      `git worktree add --detach <tmp> <rev>` and extracts
    ///      from the tmp tree. (Issue #37 / RFC-032 §A1.6.)
    ///
    ///   2. `<url>@<sha>` — remote: clones `<url>` into a persistent
    ///      cache at `$CFDB_CACHE_DIR` (or `$XDG_CACHE_HOME/cfdb/extract`
    ///      or `$HOME/.cache/cfdb/extract`), checks out `<sha>`, and
    ///      extracts. Auth inherits ambient git credentials. Accepted
    ///      URL schemes: `http://`, `https://`, `ssh://`, `file://`.
    ///      (Issue #96 / RFC-cfdb.md Addendum B §A1.7, Option W
    ///      bilateral drift-lock.)
    #[arg(long)]
    pub rev: Option<String>,
}

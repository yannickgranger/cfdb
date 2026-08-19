use std::path::PathBuf;

use clap::Args;

#[derive(Debug, Args)]
#[command(about = "Extract facts from a Rust workspace into a keyspace on disk")]
pub(crate) struct ExtractArgs {
    #[arg(
        help = "Root of the target Rust workspace (must contain Cargo.toml). When `--rev` is passed, this is the git repository root and extraction walks a temporary worktree checked out at `<rev>` rather than the live tree"
    )]
    #[arg(long)]
    pub workspace: PathBuf,
    #[arg(help = "Directory to write the per-keyspace JSON files into")]
    #[arg(long)]
    pub db: PathBuf,
    #[arg(
        help = "Keyspace name. Defaults to the basename of `--workspace` (or the short `<rev>` when `--rev` is passed without an explicit keyspace)"
    )]
    #[arg(long)]
    pub keyspace: Option<String>,
    #[arg(
        help = "Run the HIR-based extractor after syn to add resolved `:CallSite`, `CALLS`, `INVOKES_AT`, `:EntryPoint`, and `EXPOSES` facts. Requires the `hir` Cargo feature"
    )]
    #[arg(long)]
    pub hir: bool,
    #[arg(
        help = "Disable the proc-macro server during HIR extraction. By default (`--hir` without this flag) cfdb passes `ProcMacroServerChoice::Sysroot` to `ra_ap_load_cargo`, raising receiver-type-resolution recall on `#[async_trait]` / `#[derive(Builder)]` / `#[tokio::test]` / cucumber-step chains. Only meaningful with `--hir`"
    )]
    #[arg(long)]
    pub no_proc_macro: bool,
    #[arg(
        help = "Extract against a specific git revision. Accepts two forms:",
        long_help = "Extract against a specific git revision. Accepts two forms:

1. `<sha|tag|branch>` — same-repo: requires `--workspace` to point at a git repository root; shells out to `git worktree add --detach <tmp> <rev>` and extracts from the tmp tree.

2. `<url>@<sha>` — remote: clones `<url>` into a persistent cache at `$CFDB_CACHE_DIR` (or `$XDG_CACHE_HOME/cfdb/extract` or `$HOME/.cache/cfdb/extract`), checks out `<sha>`, and extracts. Auth inherits ambient git credentials. Accepted URL schemes: `http://`, `https://`, `ssh://`, `file://`."
    )]
    #[arg(long)]
    pub rev: Option<String>,
    #[arg(
        help = "Emit a per-phase wall-clock breakdown of the extract to stderr after it completes — `{cargo-metadata, syn-walk, deferred-resolve, ingest, hir-load (if --hir), save}`. Diagnostic only: the timings go to stderr, never into the keyspace or its determinism hash, so the extracted graph is byte-identical with or without this flag"
    )]
    #[arg(long)]
    pub profile: bool,
}

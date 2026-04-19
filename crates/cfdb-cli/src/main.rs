//! `cfdb` — CLI wire form for cfdb v0.1 (RFC §6.2 / §11).
//!
//! Exposes the full 17-verb cfdb API surface (RFC §6 + council-cfdb-wiring
//! RATIFIED §A.14 + §A.17) as clap subcommands:
//!
//! INGEST (5):
//! - `cfdb extract --workspace <path> --db <path> [--keyspace <name>]`
//! - `cfdb enrich-docs --db <path> --keyspace <name>`             (Phase A stub)
//! - `cfdb enrich-metrics --db <path> --keyspace <name>`          (Phase A stub)
//! - `cfdb enrich-history --db <path> --keyspace <name>`          (Phase A stub)
//! - `cfdb enrich-concepts --db <path> --keyspace <name>`         (Phase A stub)
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

use std::path::PathBuf;
use std::process::ExitCode;

use cfdb_cli::{
    diff, drop_keyspace_cmd, dump, enrich, export, extract, list_callers, list_items_matching,
    list_keyspaces, query, schema_describe_cmd, scope, snapshots, typed_stub, violations,
    CfdbCliError, EnrichVerb,
};
use cfdb_core::{ItemKind, UnknownItemKind};
use clap::{Parser, Subcommand};

/// clap value parser for a single `--kinds` entry. Delegates to
/// [`ItemKind::from_str`] so the CLI surface is bound to the council-ratified
/// vocabulary; unknown values exit with code 2 (clap default for value
/// parser errors).
fn parse_item_kind(s: &str) -> Result<ItemKind, UnknownItemKind> {
    s.parse::<ItemKind>()
}

#[derive(Debug, Parser)]
#[command(name = "cfdb", version, about = "code facts database")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Print cfdb version + schema version.
    Version,

    /// Extract facts from a Rust workspace into a keyspace on disk.
    Extract {
        /// Root of the target Rust workspace (must contain Cargo.toml).
        #[arg(long)]
        workspace: PathBuf,
        /// Directory to write the per-keyspace JSON files into.
        #[arg(long)]
        db: PathBuf,
        /// Keyspace name. Defaults to the basename of `--workspace`.
        #[arg(long)]
        keyspace: Option<String>,
    },

    /// Run a Cypher-subset query against a loaded keyspace.
    Query {
        /// Directory containing per-keyspace JSON files.
        #[arg(long)]
        db: PathBuf,
        /// Keyspace to query.
        #[arg(long)]
        keyspace: String,
        /// The Cypher-subset query source.
        cypher: String,
        /// Inline JSON object of parameter substitutions, e.g.
        /// `--params '{"crate":"cfdb-core"}'`. Phase A: parsed but not yet
        /// threaded through the evaluator (RFC §6.2 — wire form first).
        #[arg(long)]
        params: Option<String>,
        /// Path to a YAML file providing the `sets?` external buckets used
        /// by `query_with_input` patterns (e.g. raid plans). Phase A:
        /// accepted but not yet wired (RFC §6.2 — wire form first).
        #[arg(long)]
        input: Option<PathBuf>,
    },

    /// Enrich a keyspace with documentation facts. Phase A stub — returns
    /// an `EnrichReport` flagged as not-implemented per RFC §6 / EPIC #3622.
    EnrichDocs {
        #[arg(long)]
        db: PathBuf,
        #[arg(long)]
        keyspace: String,
    },

    /// Enrich a keyspace with quality-signal facts (complexity, unwraps,
    /// clones-in-loops). Phase A stub.
    EnrichMetrics {
        #[arg(long)]
        db: PathBuf,
        #[arg(long)]
        keyspace: String,
    },

    /// Enrich a keyspace with git-history facts (last-touched, churn,
    /// author). Phase A stub.
    EnrichHistory {
        #[arg(long)]
        db: PathBuf,
        #[arg(long)]
        keyspace: String,
    },

    /// Enrich a keyspace with bounded-context / concept facts. Phase A stub.
    EnrichConcepts {
        #[arg(long)]
        db: PathBuf,
        #[arg(long)]
        keyspace: String,
    },

    /// Typed verb — find the canonical definition of a concept.
    /// Convenience composer over `query_raw` (RFC §6 TYPED). Phase A stub.
    FindCanonical {
        #[arg(long)]
        db: PathBuf,
        #[arg(long)]
        keyspace: String,
        #[arg(long)]
        concept: String,
    },

    /// Typed verb — list callers of a fully-qualified name (regex pattern).
    /// Convenience composer over `query_raw` that binds the embedded
    /// `list-callers.cypher` template with `$qname = --qname`.
    ListCallers {
        #[arg(long)]
        db: PathBuf,
        #[arg(long)]
        keyspace: String,
        #[arg(long)]
        qname: String,
    },

    /// Typed verb — list bypasses of the canonical definition of a concept.
    /// Convenience composer over `query_raw`. Phase A stub.
    ListBypasses {
        #[arg(long)]
        db: PathBuf,
        #[arg(long)]
        keyspace: String,
        #[arg(long)]
        concept: String,
    },

    /// Typed verb — list `:Item` nodes whose `name` matches a regex, with
    /// optional kind filter and optional group-by-bounded_context
    /// partitioning (council-cfdb-wiring RATIFIED §A.14). Subsumes the
    /// three R1 proposals `list_context_owner` / `list_definitions_of` /
    /// `list_items_matching` via a parameterized filter composed over the
    /// existing `:Item` substrate. Syn-level only — no HIR dependency.
    ListItemsMatching {
        #[arg(long)]
        db: PathBuf,
        #[arg(long)]
        keyspace: String,
        /// openCypher-compatible regex applied to `:Item.name`.
        #[arg(long)]
        name_pattern: String,
        /// Optional comma-separated list of Item kinds. Accepted values:
        /// `Struct`, `Enum`, `Fn`, `Const`, `TypeAlias`, `ImplBlock`, `Trait`
        /// (the 7 council-ratified names). Unknown values exit with code 2.
        #[arg(long, value_delimiter = ',', value_parser = parse_item_kind)]
        kinds: Option<Vec<ItemKind>>,
        /// When set, results are grouped by `:Item.bounded_context` with a
        /// `COLLECT` of matching items per group (subsumption target for
        /// ddd's `list_context_owner`).
        #[arg(long)]
        group_by_context: bool,
    },

    /// Emit the structured infection inventory (§A3.3 shape) for a bounded
    /// context. Pure data aggregation — JSON only, no raid-plan prose
    /// (council-cfdb-wiring RATIFIED §A.17). Output is tier-3 ephemeral;
    /// consumer skills (`/operate-module`, `/boy-scout --from-inventory`)
    /// read and format it.
    Scope {
        #[arg(long)]
        db: PathBuf,
        /// Required bounded-context name; filters to items where
        /// `Item.bounded_context == <name>`. Unknown context → exit 1 with
        /// "known contexts: ..." message.
        #[arg(long)]
        context: String,
        /// Optional workspace path (reserved for default-keyspace
        /// resolution; v0.1 requires `--keyspace` to select the keyspace
        /// when the db directory holds more than one).
        #[arg(long)]
        workspace: Option<PathBuf>,
        /// Output format. v0.1 supports only `json`; `table` is deferred
        /// to v0.2 (§A3.3) and exits 2 with an explanatory message.
        #[arg(long, default_value = "json")]
        format: String,
        /// Write to file path; otherwise stdout.
        #[arg(long)]
        output: Option<PathBuf>,
        /// Keyspace name (defaults to `cfdb-v01` if the db directory
        /// contains exactly one keyspace file; otherwise required).
        #[arg(long)]
        keyspace: Option<String>,
    },

    /// List snapshots in a database. v0.1 maps each on-disk keyspace to one
    /// snapshot; sha/timestamp/schema_version columns are populated as
    /// available (Phase A: keyspace + schema_version only).
    Snapshots {
        #[arg(long)]
        db: PathBuf,
    },

    /// Diff two keyspaces (added / removed / changed facts). Phase A stub —
    /// the snapshot diff verb ships with the snapshot store in Phase B.
    Diff {
        #[arg(long)]
        db: PathBuf,
        /// First keyspace (the "before" snapshot).
        #[arg(long)]
        a: String,
        /// Second keyspace (the "after" snapshot).
        #[arg(long)]
        b: String,
        /// Optional comma-separated list of fact kinds to diff
        /// (e.g. `nodes,edges`). Phase A: parsed but not yet wired.
        #[arg(long)]
        kinds: Option<String>,
    },

    /// Drop a keyspace from the database. The only deletion verb (RFC §6 G5).
    Drop {
        #[arg(long)]
        db: PathBuf,
        #[arg(long)]
        keyspace: String,
    },

    /// Print the canonical SchemaDescribe (node labels, edge labels,
    /// per-attribute provenance) as pretty JSON. Read-only, deterministic.
    SchemaDescribe,

    /// Export a keyspace in the requested wire format. v0.1 supports the
    /// canonical sorted JSONL dump (the same output as `cfdb dump`); the
    /// `--format` flag exists for forward-compat with future formats.
    Export {
        #[arg(long)]
        db: PathBuf,
        #[arg(long)]
        keyspace: String,
        /// Output format. v0.1 only supports `sorted-jsonl`.
        #[arg(long, default_value = "sorted-jsonl")]
        format: String,
    },

    /// Run a rule file and exit 1 if any violations are found.
    ///
    /// Intended as the drop-in replacement for handwritten Rust
    /// architecture tests. Architecture tests in qbot-core can be
    /// expressed as one `.cypher` rule file plus a one-liner shell
    /// test that runs `cfdb violations --rule path.cypher` and fails
    /// on exit code 1.
    Violations {
        /// Directory containing per-keyspace JSON files.
        #[arg(long)]
        db: PathBuf,
        /// Keyspace to query.
        #[arg(long)]
        keyspace: String,
        /// Path to a `.cypher` rule file. Each row in the result set is
        /// a violation.
        #[arg(long)]
        rule: PathBuf,
        /// Always exit 0, even when violations are found. Useful for
        /// inventorying current state without failing CI.
        #[arg(long)]
        no_fail: bool,
        /// Emit only the integer row count on stdout, suppressing the
        /// pretty-JSON payload. Intended for CI scripts like
        /// `ci/cross-dogfood.sh` (RFC-033 §3.2) that capture the count
        /// via `rows=$(cfdb violations ... --count-only --no-fail)` and
        /// tally findings across rules. Stderr is unchanged.
        #[arg(long)]
        count_only: bool,
    },

    /// Print the canonical sorted dump of a keyspace.
    Dump {
        #[arg(long)]
        db: PathBuf,
        #[arg(long)]
        keyspace: String,
    },

    /// List keyspaces discoverable in a database directory.
    ListKeyspaces {
        #[arg(long)]
        db: PathBuf,
    },
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
        Command::Version => {
            println!("cfdb {}", env!("CARGO_PKG_VERSION"));
            println!("schema {}", cfdb_core::SchemaVersion::CURRENT);
        }
        Command::Extract {
            workspace,
            db,
            keyspace,
        } => extract(workspace, db, keyspace)?,
        Command::Query {
            db,
            keyspace,
            cypher,
            params,
            input,
        } => query(db, keyspace, cypher, params, input)?,
        Command::Violations {
            db,
            keyspace,
            rule,
            no_fail,
            count_only,
        } => {
            let rows_found = violations(db, keyspace, rule, count_only)?;
            if rows_found > 0 && !no_fail {
                std::process::exit(1);
            }
        }
        Command::Dump { db, keyspace } => dump(db, keyspace)?,
        Command::Export {
            db,
            keyspace,
            format,
        } => export(db, keyspace, &format)?,
        Command::ListKeyspaces { db } => list_keyspaces(db)?,
        Command::EnrichDocs { db, keyspace } => enrich(db, keyspace, EnrichVerb::Docs)?,
        Command::EnrichMetrics { db, keyspace } => enrich(db, keyspace, EnrichVerb::Metrics)?,
        Command::EnrichHistory { db, keyspace } => enrich(db, keyspace, EnrichVerb::History)?,
        Command::EnrichConcepts { db, keyspace } => enrich(db, keyspace, EnrichVerb::Concepts)?,
        Command::FindCanonical {
            db,
            keyspace,
            concept,
        } => typed_stub("find_canonical", &db, &keyspace, &[("concept", &concept)])?,
        Command::ListCallers {
            db,
            keyspace,
            qname,
        } => list_callers(db, keyspace, qname)?,
        Command::ListBypasses {
            db,
            keyspace,
            concept,
        } => typed_stub("list_bypasses", &db, &keyspace, &[("concept", &concept)])?,
        Command::ListItemsMatching {
            db,
            keyspace,
            name_pattern,
            kinds,
            group_by_context,
        } => list_items_matching(
            &db,
            &keyspace,
            &name_pattern,
            kinds.as_deref(),
            group_by_context,
        )?,
        Command::Scope {
            db,
            context,
            workspace,
            format,
            output,
            keyspace,
        } => scope(
            &db,
            &context,
            workspace.as_deref(),
            &format,
            output.as_deref(),
            keyspace.as_deref(),
        )?,
        Command::Snapshots { db } => snapshots(db)?,
        Command::Diff { db, a, b, kinds } => diff(db, a, b, kinds)?,
        Command::Drop { db, keyspace } => drop_keyspace_cmd(db, keyspace)?,
        Command::SchemaDescribe => schema_describe_cmd()?,
    }
    Ok(())
}

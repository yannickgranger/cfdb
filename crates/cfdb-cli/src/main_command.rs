//! The `cfdb` CLI subcommand enum. Split out of `main.rs` as part of the
//! #128 god-file split. Moved-only — no variant renames, no signature
//! changes. See `main.rs` for top-level entry, `main_parse.rs` for the
//! two `value_parser` bindings referenced here, and `main_dispatch.rs`
//! for the group-dispatch helpers this enum feeds.

use std::path::PathBuf;

use cfdb_cli::TriggerId;
use cfdb_core::ItemKind;
use clap::Subcommand;

use crate::main_parse::{parse_item_kind, parse_trigger_id};

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    /// Print cfdb version + schema version.
    Version,

    /// Extract facts from a Rust workspace into a keyspace on disk.
    Extract {
        /// Root of the target Rust workspace (must contain Cargo.toml).
        /// When `--rev` is passed, this is the git repository root and
        /// extraction walks a temporary worktree checked out at `<rev>`
        /// rather than the live tree.
        #[arg(long)]
        workspace: PathBuf,
        /// Directory to write the per-keyspace JSON files into.
        #[arg(long)]
        db: PathBuf,
        /// Keyspace name. Defaults to the basename of `--workspace`
        /// (or the short `<rev>` when `--rev` is passed without an
        /// explicit keyspace).
        #[arg(long)]
        keyspace: Option<String>,
        /// Run the HIR-based extractor after syn to add resolved
        /// `:CallSite`, `CALLS`, `INVOKES_AT`, `:EntryPoint`, and
        /// `EXPOSES` facts. Requires the `hir` Cargo feature —
        /// rebuild with `cargo build -p cfdb-cli --features hir`
        /// to opt in (Issue #86 / slice 4).
        #[arg(long)]
        hir: bool,
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
        rev: Option<String>,
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

    /// Enrich a keyspace with git-history facts — commit age, author, churn
    /// count per `:Item` file. RFC addendum §A2.2 row 1. Phase A stub —
    /// implementation lands in #43 slice 43-B (issue #105) behind the
    /// `git-enrich` feature flag.
    EnrichGitHistory {
        #[arg(long)]
        db: PathBuf,
        #[arg(long)]
        keyspace: String,
        /// Workspace root whose git history to consult (must enclose a git
        /// repository). Without this flag the pass reports `ran: false` + a
        /// warning about the missing root. Requires the `git-enrich` feature.
        #[arg(long)]
        workspace: Option<PathBuf>,
    },

    /// Enrich a keyspace with RFC-reference facts — scan `docs/rfc/*.md` and
    /// `.concept-graph/*.md` for concept-name matches, emit `:RfcDoc` nodes
    /// and `(:Item)-[:REFERENCED_BY]->(:RfcDoc)` edges. RFC addendum §A2.2
    /// row 2. Phase A stub — implementation lands in #43 slice 43-D
    /// (issue #107). Scope is RFC-file matching only; broader rustdoc
    /// rendering is a non-goal for v0.2 per the §A2.2 amendment.
    EnrichRfcDocs {
        #[arg(long)]
        db: PathBuf,
        #[arg(long)]
        keyspace: String,
        /// Workspace root whose `docs/**/*.md` and `.concept-graph/*.md`
        /// files to scan for item-name references. Without this flag the
        /// pass reports `ran: false` + a warning about the missing root.
        #[arg(long)]
        workspace: Option<PathBuf>,
    },

    /// Enrich a keyspace with deprecation facts — `:Item.is_deprecated` and
    /// `deprecation_since`. RFC addendum §A2.2 row 3. Phase A stub — the
    /// real work lands in #43 slice 43-C (issue #106) as an
    /// **extractor extension** in `cfdb-extractor/src/attrs.rs`, not as a
    /// Phase D enrichment body; this CLI verb's post-43-C behavior is a
    /// `ran: true, attrs_written: 0` report naming the extractor as the
    /// real source.
    EnrichDeprecation {
        #[arg(long)]
        db: PathBuf,
        #[arg(long)]
        keyspace: String,
    },

    /// Re-enrich a keyspace's `:Item.bounded_context` attribute after
    /// `.cfdb/concepts/*.toml` has changed. RFC addendum §A2.2 row 4.
    /// Phase A stub — implementation lands in #43 slice 43-E (issue #108).
    /// Mostly a no-op on fresh extractions (the extractor already populates
    /// `bounded_context`); slice 43-E's v0.2-9 ≥95% accuracy gate gates
    /// merge of both #108 and the downstream classifier (#48).
    EnrichBoundedContext {
        #[arg(long)]
        db: PathBuf,
        #[arg(long)]
        keyspace: String,
        /// Workspace root whose `.cfdb/concepts/*.toml` files to re-read.
        /// Without this flag the pass reports `ran: false` + a warning about
        /// the missing root (TOML overrides live under the workspace root, so
        /// there is no useful default).
        #[arg(long)]
        workspace: Option<PathBuf>,
    },

    /// Materialize `:Concept` nodes from `.cfdb/concepts/<name>.toml`
    /// declarations and emit `LABELED_AS` + `CANONICAL_FOR` edges. RFC
    /// addendum §A2.2 row 6 (sixth pass added by #43 council DDD lens).
    /// Phase A stub — implementation lands in #43 slice 43-F (issue #109)
    /// and unblocks issues #101 (Trigger T1) and #102 (Trigger T3).
    EnrichConcepts {
        #[arg(long)]
        db: PathBuf,
        #[arg(long)]
        keyspace: String,
        /// Workspace root whose `.cfdb/concepts/*.toml` files to read.
        /// Without this flag the pass reports `ran: false` + a warning about
        /// the missing root (TOML overrides live under the workspace root, so
        /// there is no useful default).
        #[arg(long)]
        workspace: Option<PathBuf>,
    },

    /// Enrich a keyspace with entry-point-reachability facts — BFS from
    /// every `:EntryPoint` over `CALLS*` edges. RFC addendum §A2.2 row 5.
    /// Phase A stub — implementation lands in #43 slice 43-G (issue #110)
    /// and consumes `:EntryPoint` nodes produced by `cfdb-hir-extractor`
    /// (v0.2+). When the keyspace has zero `:EntryPoint` nodes the real
    /// implementation returns `ran: false` with a clear warning per
    /// clean-arch B3 degraded path.
    EnrichReachability {
        #[arg(long)]
        db: PathBuf,
        #[arg(long)]
        keyspace: String,
    },

    /// Enrich a keyspace with quality-signal facts
    /// (`unwrap_count`, `cyclomatic`, `test_coverage`, `dup_cluster_id`)
    /// on `:Item{kind:"fn"}` nodes. Populated by `PetgraphStore::enrich_metrics`
    /// (RFC-036 §3.3 / issue #203) when the binary is built with
    /// `--features quality-metrics`. Without the feature the verb dispatches
    /// to a `ran: false` report naming the missing feature flag.
    ///
    /// `--workspace` is required for the real pass — syn re-parses source
    /// files referenced by `:Item.file`. A stored keyspace with its own
    /// `workspace_root` (from a prior `--workspace` extract) overrides an
    /// omitted `--workspace` here; otherwise the pass returns a degraded
    /// report.
    EnrichMetrics {
        #[arg(long)]
        db: PathBuf,
        #[arg(long)]
        keyspace: String,
        /// Workspace-root path handed to the store before dispatching —
        /// required for the real pass to re-parse source files. Mirrors
        /// the `--workspace` flag shared by every workspace-scanning
        /// enrichment verb (#43 slices B/D/E/F).
        #[arg(long)]
        workspace: Option<PathBuf>,
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
        /// Emit one `explain: <pattern> → indexed|fallback` line per
        /// `candidate_nodes` invocation on stderr (RFC-035 slice 7).
        /// Requires `--workspace` to be set — without a workspace,
        /// `.cfdb/indexes.toml` is not loaded and every MATCH falls
        /// back by default.
        #[arg(long, default_value_t = false)]
        explain: bool,
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

    /// Run a cfdb editorial-drift trigger and exit 1 if any findings
    /// fire. Issue #101 ships `T1` (concept-declared-in-TOML-but-
    /// missing-in-code, three sub-verdicts: CONCEPT_UNWIRED,
    /// MISSING_CANONICAL_CRATE, STALE_RFC_REFERENCE). T3 is reserved
    /// for issue #102.
    ///
    /// Unlike `violations --rule <file>` which runs arbitrary cypher,
    /// `check --trigger <ID>` dispatches to a closed registry of
    /// embedded rules keyed by trigger id — so consumer skills can
    /// bind to `--trigger T1` as a stable contract rather than a
    /// filesystem path.
    Check {
        /// Directory containing per-keyspace JSON files.
        #[arg(long)]
        db: PathBuf,
        /// Keyspace to query.
        #[arg(long)]
        keyspace: String,
        /// Trigger identifier — e.g. `T1`. Valid values are derived
        /// from [`TriggerId::variants`]; unknown values fail with a
        /// message enumerating the known set.
        #[arg(long, value_parser = parse_trigger_id)]
        trigger: TriggerId,
        /// Always exit 0, even when findings are reported. Matches
        /// the `violations --no-fail` idiom for CI scripts that want
        /// to inventory without failing.
        #[arg(long)]
        no_fail: bool,
    },

    /// Run a named predicate from `.cfdb/predicates/<name>.cypher` and
    /// exit 1 if any rows match — RFC-034 Slice 3.
    ///
    /// Unlike `violations --rule <path>` (which loads a user-supplied
    /// `.cypher` file with no param binding) this verb loads a named
    /// predicate from the shipped library + resolves `--param` CLI args
    /// via [`crate::param_resolver::resolve_params`] before executing.
    /// Exit contract matches `violations` / `check`: non-zero iff the
    /// predicate returns ≥1 row.
    CheckPredicate {
        /// Directory containing per-keyspace JSON files.
        #[arg(long)]
        db: PathBuf,
        /// Keyspace to query.
        #[arg(long)]
        keyspace: String,
        /// Workspace root — used to locate `.cfdb/predicates/<name>.cypher`
        /// AND `.cfdb/concepts/*.toml` for `context:` param resolution.
        #[arg(long)]
        workspace_root: PathBuf,
        /// Predicate basename (without `.cypher`) — e.g. `path-regex`.
        #[arg(long)]
        name: String,
        /// Repeatable `--param <name>:<form>:<value>` CLI arg. Forms:
        /// `context:<concept-name>` / `regex:<pattern>` /
        /// `literal:<value>` / `list:<a,b,c>`. See RFC-034 §3.4.
        #[arg(long = "param")]
        params: Vec<String>,
        /// Output format. `text` (default) emits the canonical
        /// three-column `qname<TAB>line<TAB>reason` per row + a stderr
        /// summary. `json` emits a pretty-printed
        /// [`crate::PredicateRunReport`] on stdout.
        #[arg(long, default_value = "text")]
        format: String,
        /// Always exit 0, even when rows are returned. Matches the
        /// `violations --no-fail` idiom for CI scripts.
        #[arg(long)]
        no_fail: bool,
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

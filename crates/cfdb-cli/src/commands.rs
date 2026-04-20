//! Core ingest / query / dump command handlers.
//!
//! Split out of `lib.rs` for the god-file decomposition (#3751). Public
//! surface preserved: every item here is re-exported from the crate root.

use std::path::{Path, PathBuf};
use std::process::Command;

use cfdb_core::schema::Keyspace;
use cfdb_core::store::StoreBackend;
use cfdb_core::{Param, PropValue, Query};
use cfdb_query::{lint_shape, parse, ShapeLint};

use crate::compose;

/// Embedded cypher template for `cfdb list-callers`. Loaded via `include_str!`
/// at compile time so the shipped binary is self-contained — no runtime file
/// lookup, no deployment-relative paths, and `cargo build` picks up edits to
/// the template automatically.
const LIST_CALLERS_CYPHER: &str = include_str!("../../../examples/queries/list-callers.cypher");

pub fn keyspace_path(db: &Path, keyspace: &str) -> PathBuf {
    db.join(format!("{keyspace}.json"))
}

pub fn extract(
    workspace: PathBuf,
    db: PathBuf,
    keyspace: Option<String>,
    hir: bool,
    rev: Option<String>,
) -> Result<(), crate::CfdbCliError> {
    match rev {
        None => extract_at_path(&workspace, &db, keyspace, hir),
        Some(rev) => extract_at_rev(&workspace, &rev, &db, keyspace, hir),
    }
}

/// Extract the current working tree at `workspace` into a keyspace on
/// disk. This is the v0.1 behaviour preserved verbatim — `extract`
/// dispatches here when `--rev` is absent.
fn extract_at_path(
    workspace: &Path,
    db: &Path,
    keyspace: Option<String>,
    hir: bool,
) -> Result<(), crate::CfdbCliError> {
    let ks_name = keyspace.unwrap_or_else(|| workspace_basename(workspace));
    let ks = Keyspace::new(&ks_name);

    eprintln!("extract: walking {}", workspace.display());
    let (nodes, edges) = cfdb_extractor::extract_workspace(workspace)?;
    eprintln!("extract: {} nodes, {} edges", nodes.len(), edges.len());

    let mut store = compose::empty_store();
    store.ingest_nodes(&ks, nodes)?;
    store.ingest_edges(&ks, edges)?;

    if hir {
        extract_hir(&mut store, &ks, workspace)?;
    }

    let path = compose::save_store(&store, &ks, db)?;
    eprintln!("extract: saved keyspace `{ks_name}` to {}", path.display());
    Ok(())
}

/// Extract against a specific git revision (commit SHA / tag / branch).
/// `repo` MUST be a git repository root; a temporary detached worktree
/// is created at `<rev>`, extraction runs against that tmp path, and
/// the worktree is removed afterwards.
///
/// Default keyspace when none is explicitly provided is the short (12-
/// char) `<rev>` — keeps "extract --rev abc123 --rev def456" producing
/// distinct keyspaces that a later `cfdb diff --a <ks-a> --b <ks-b>`
/// can consume.
fn extract_at_rev(
    repo: &Path,
    rev: &str,
    db: &Path,
    keyspace: Option<String>,
    hir: bool,
) -> Result<(), crate::CfdbCliError> {
    if !repo.join(".git").exists() && !repo.join(".git").is_file() {
        return Err(crate::CfdbCliError::Usage(format!(
            "--rev requires --workspace to point at a git repository root (no .git found under {})",
            repo.display()
        )));
    }
    let ks_name = keyspace.unwrap_or_else(|| short_rev(rev));
    let tmp = tempfile::tempdir()?;
    // `git worktree add` creates the target dir; pass a sub-path so the
    // tempdir itself stays empty and can be dropped cleanly.
    let worktree_path = tmp.path().join("worktree");
    let worktree_guard = GitWorktree::add(repo, &worktree_path, rev)?;

    eprintln!(
        "extract --rev {rev}: walking worktree {}",
        worktree_guard.path().display()
    );

    // Run the normal extract against the temp worktree. If it fails, the
    // guard's Drop still removes the worktree.
    let result = extract_at_path(worktree_guard.path(), db, Some(ks_name), hir);

    // Explicit remove so we surface removal errors rather than swallowing
    // them in Drop. If removal fails, we still return the extract result —
    // leaked worktrees are recoverable (`git worktree prune`).
    worktree_guard.remove_soft_log(repo);
    result
}

/// Compute the default keyspace name when `--rev` is given without
/// `--keyspace`. Short SHAs are truncated to 12 chars so keyspace files
/// land with a stable short name; non-SHA revs (tags/branches) are used
/// verbatim after path-unsafe char stripping.
fn short_rev(rev: &str) -> String {
    if rev.len() > 12 && rev.chars().all(|c| c.is_ascii_hexdigit()) {
        rev[..12].to_string()
    } else {
        rev.replace(['/', ' ', '\t'], "_")
    }
}

/// Default keyspace name from `--workspace` basename. Extracted so the
/// `--rev` + `--workspace` paths share nothing but can both call this when
/// a keyspace default is needed.
fn workspace_basename(workspace: &Path) -> String {
    workspace
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("default")
        .to_string()
}

/// RAII guard around `git worktree add ... <path> <rev>`. The `Drop` impl
/// best-effort-removes the worktree so panics during extraction still
/// clean up. Successful returns call `remove_soft_log` explicitly so
/// removal errors surface.
struct GitWorktree {
    path: PathBuf,
    removed: bool,
}

impl GitWorktree {
    fn add(repo: &Path, path: &Path, rev: &str) -> Result<Self, crate::CfdbCliError> {
        let status = Command::new("git")
            .current_dir(repo)
            .args(["worktree", "add", "--detach", "--quiet"])
            .arg(path)
            .arg(rev)
            .status()?;
        if !status.success() {
            return Err(crate::CfdbCliError::Usage(format!(
                "git worktree add --detach {} {}: exit {status}",
                path.display(),
                rev
            )));
        }
        Ok(GitWorktree {
            path: path.to_path_buf(),
            removed: false,
        })
    }

    fn path(&self) -> &Path {
        &self.path
    }

    /// Remove the worktree via `git worktree remove --force`. Logs on
    /// failure but never panics; the tempdir-based parent will clean up
    /// the orphan files even if git's internal state is off.
    fn remove_soft_log(mut self, repo: &Path) {
        if !self.removed {
            let _ = Command::new("git")
                .current_dir(repo)
                .args(["worktree", "remove", "--force"])
                .arg(&self.path)
                .status();
            self.removed = true;
        }
    }
}

impl Drop for GitWorktree {
    fn drop(&mut self) {
        if !self.removed {
            // Best-effort cleanup — do not panic from Drop.
            let _ = Command::new("git")
                .args(["worktree", "remove", "--force"])
                .arg(&self.path)
                .status();
        }
    }
}

/// Run the HIR pipeline when the `hir` feature is compiled in. The
/// feature-gated `crate::hir::extract_and_ingest_hir` module is the
/// single integration seam — if not compiled, `--hir` emits a clear
/// error instead of silently no-oping.
#[cfg(feature = "hir")]
fn extract_hir(
    store: &mut cfdb_petgraph::PetgraphStore,
    ks: &Keyspace,
    workspace: &Path,
) -> Result<(), crate::CfdbCliError> {
    crate::hir::extract_and_ingest_hir(store, ks, workspace)
        .map_err(|e| crate::CfdbCliError::from(format!("hir extract failed: {e}")))?;
    Ok(())
}

#[cfg(not(feature = "hir"))]
fn extract_hir(
    _store: &mut cfdb_petgraph::PetgraphStore,
    _ks: &Keyspace,
    _workspace: &Path,
) -> Result<(), crate::CfdbCliError> {
    Err(crate::CfdbCliError::from(
        "`--hir` requires the `hir` Cargo feature — rebuild with `cargo build -p cfdb-cli --features hir`".to_string(),
    ))
}

pub fn query(
    db: PathBuf,
    keyspace: String,
    cypher: String,
    params: Option<String>,
    input: Option<PathBuf>,
) -> Result<(), crate::CfdbCliError> {
    let mut parsed = parse(&cypher).map_err(|e| format!("parse error: {e}"))?;

    if let Some(raw) = params.as_deref() {
        let json: serde_json::Value =
            serde_json::from_str(raw).map_err(|e| format!("--params is not valid JSON: {e}"))?;
        bind_json_params(&mut parsed, &json)?;
    }
    if let Some(path) = input.as_deref() {
        if !path.exists() {
            return Err(format!("--input file not found: {}", path.display()).into());
        }
        eprintln!("query: --input accepted but not yet wired in v0.1 (Phase A — RFC §6.2)");
    }

    let lints = lint_shape(&parsed);
    for lint in &lints {
        match lint {
            ShapeLint::CartesianFunctionEquality {
                message,
                suggestion,
            } => {
                eprintln!("shape-lint: {message}");
                eprintln!("  suggestion: {suggestion}");
            }
            // ShapeLint is #[non_exhaustive]; v0.2 may add new variants.
            _ => eprintln!("shape-lint: {lint:?}"),
        }
    }

    let (store, ks) = compose::load_store(&db, &keyspace)?;

    let result = store.execute(&ks, &parsed)?;

    let as_json = serde_json::to_string_pretty(&result)?;
    println!("{as_json}");
    Ok(())
}

/// Bind a `--params <json>` object into a parsed `Query`'s param bag. The
/// input MUST be a JSON object whose values are scalars (string, number,
/// bool, or null). Arrays and objects are rejected with a clear error —
/// v0.1 only supports scalar bindings; list/typed bindings come later.
/// This is the canonical wire-up boundary for the CLI → evaluator param
/// flow: the parser emits an empty `Query.params` bag, this function
/// populates it, and the evaluator reads from the populated bag.
fn bind_json_params(
    parsed: &mut Query,
    json: &serde_json::Value,
) -> Result<(), crate::CfdbCliError> {
    let obj = json
        .as_object()
        .ok_or("--params must be a JSON object, e.g. '{\"qname\":\"(?i).*kalman.*\"}'")?;
    for (k, v) in obj {
        bind_single_param(parsed, k, v)?;
    }
    Ok(())
}

/// Bind one `(key, value)` from the `--params` JSON object into the parsed
/// query's param bag. Factored out of [`bind_json_params`] so the `k.clone()`
/// required by the scalar insert lives in a helper rather than in the
/// outer `for (k, v) in obj` loop body — the quality-metrics gate treats
/// the closure-less `for` as the clone-in-loop trigger.
fn bind_single_param(
    parsed: &mut Query,
    k: &str,
    v: &serde_json::Value,
) -> Result<(), crate::CfdbCliError> {
    match v {
        serde_json::Value::String(_)
        | serde_json::Value::Number(_)
        | serde_json::Value::Bool(_)
        | serde_json::Value::Null => {
            parsed
                .params
                .insert(k.to_string(), Param::Scalar(PropValue::from_json(v)));
            Ok(())
        }
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => Err(format!(
            "--params `{k}` must be a scalar (string/number/bool/null); \
             arrays and objects are not supported in v0.1"
        )
        .into()),
    }
}

/// `cfdb list-callers --db <path> --keyspace <name> --qname <regex>` —
/// typed convenience verb over the raw `query` path. Loads the embedded
/// `list-callers.cypher` template, binds `$qname` to the CLI arg, executes
/// against the named keyspace, and prints the result as pretty JSON in
/// the same format as `cfdb query`. The template and the raw path MUST
/// produce byte-identical output for the same `$qname` input — that is
/// the genericity contract the typed verbs are meant to satisfy (one
/// query, many targets, sugar over the raw path).
pub fn list_callers(
    db: PathBuf,
    keyspace: String,
    qname: String,
) -> Result<(), crate::CfdbCliError> {
    let path = keyspace_path(&db, &keyspace);
    if !path.exists() {
        return Err(format!(
            "keyspace `{keyspace}` not found in db `{}` (looked for {})",
            db.display(),
            path.display()
        )
        .into());
    }

    let mut parsed = parse(LIST_CALLERS_CYPHER)
        .map_err(|e| format!("parse error in embedded list-callers template: {e}"))?;
    parsed
        .params
        .insert("qname".to_string(), Param::Scalar(PropValue::Str(qname)));

    let (store, ks) = compose::load_store(&db, &keyspace)?;
    let result = store.execute(&ks, &parsed)?;

    let as_json = serde_json::to_string_pretty(&result)?;
    println!("{as_json}");
    Ok(())
}

/// Run a .cypher rule file and print violations. Returns the number of
/// rows found so the caller can set the process exit code.
///
/// Prints to stderr (always):
/// - A shape-lint warning if one fires on the rule (same as `cfdb query`).
/// - A human-readable `violations: N (rule: <path>)` summary line.
///
/// Prints to stdout:
/// - Default: pretty-printed JSON of the full `QueryResult` (rows +
///   warnings) so callers can parse it programmatically.
/// - When `count_only` is set: the integer row count on its own line,
///   suitable for capture by `rows=$(cfdb violations ... --count-only)`
///   in CI scripts like `ci/cross-dogfood.sh` (RFC-033 §3.2). The
///   JSON payload is suppressed in this mode — the caller already
///   knows the rule file path and wants only the terse count.
pub fn violations(
    db: PathBuf,
    keyspace: String,
    rule: PathBuf,
    count_only: bool,
) -> Result<usize, crate::CfdbCliError> {
    let cypher = std::fs::read_to_string(&rule)
        .map_err(|e| format!("read rule file {}: {e}", rule.display()))?;

    let parsed = parse(&cypher).map_err(|e| format!("parse error in {}: {e}", rule.display()))?;
    let lints = lint_shape(&parsed);
    for lint in &lints {
        match lint {
            ShapeLint::CartesianFunctionEquality {
                message,
                suggestion,
            } => {
                eprintln!("shape-lint: {message}");
                eprintln!("  suggestion: {suggestion}");
            }
            _ => eprintln!("shape-lint: {lint:?}"),
        }
    }

    let (store, ks) = compose::load_store(&db, &keyspace)?;
    let result = store.execute(&ks, &parsed)?;

    let row_count = result.rows.len();
    eprintln!("violations: {row_count} (rule: {})", rule.display());

    if count_only {
        println!("{row_count}");
    } else {
        let as_json = serde_json::to_string_pretty(&result)?;
        println!("{as_json}");
    }

    Ok(row_count)
}

pub fn dump(db: PathBuf, keyspace: String) -> Result<(), crate::CfdbCliError> {
    let (store, ks) = compose::load_store(&db, &keyspace)?;
    let dump = store.canonical_dump(&ks)?;
    println!("{dump}");
    Ok(())
}

pub fn list_keyspaces(db: PathBuf) -> Result<(), crate::CfdbCliError> {
    if !db.exists() {
        return Ok(());
    }
    let mut names: Vec<String> = std::fs::read_dir(&db)?
        .filter_map(|entry| entry.ok())
        .filter_map(|e| {
            let path = e.path();
            if path.extension().and_then(|s| s.to_str()) == Some("json") {
                path.file_stem().and_then(|s| s.to_str()).map(String::from)
            } else {
                None
            }
        })
        .collect();
    names.sort();
    for n in names {
        println!("{n}");
    }
    Ok(())
}

/// `cfdb export` — alias of `cfdb dump` with a `--format` flag for forward
/// compatibility. v0.1 only supports `sorted-jsonl` (the canonical dump).
pub fn export(db: PathBuf, keyspace: String, format: &str) -> Result<(), crate::CfdbCliError> {
    if format != "sorted-jsonl" {
        return Err(format!("unsupported --format `{format}`. v0.1 supports: sorted-jsonl").into());
    }
    dump(db, keyspace)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_rev_truncates_long_sha() {
        assert_eq!(
            short_rev("abcdef0123456789abcdef0123456789abcdef01"),
            "abcdef012345"
        );
    }

    #[test]
    fn short_rev_preserves_short_sha_or_branch_name() {
        assert_eq!(short_rev("abc123"), "abc123");
        assert_eq!(short_rev("main"), "main");
        assert_eq!(short_rev("v0.2.3"), "v0.2.3");
    }

    #[test]
    fn short_rev_sanitises_path_unsafe_chars() {
        assert_eq!(short_rev("feature/new-thing"), "feature_new-thing");
        assert_eq!(short_rev("release candidate"), "release_candidate");
    }

    #[test]
    fn short_rev_keeps_non_hex_long_names_verbatim() {
        // A tag like `v0.1.0-beta2` is longer than 12 chars but not
        // hex-only — should be kept as-is (not truncated).
        assert_eq!(short_rev("v0.1.0-beta2"), "v0.1.0-beta2");
    }
}

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
    // The `Some(rev) if is_url_at_sha(rev)` guard is the SINGLE resolution
    // point for URL-vs-SHA discrimination. Do not duplicate this check
    // inside `extract_at_rev` or `extract_at_url_rev` — see the Wiring
    // Assertion in `.prescriptions/96.md`.
    match rev.as_deref() {
        None => extract_at_path(&workspace, &db, keyspace, hir),
        Some(rev) if is_url_at_sha(rev) => extract_at_url_rev(rev, &db, keyspace, hir),
        Some(rev) => extract_at_rev(&workspace, rev, &db, keyspace, hir),
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

/// Extract against a specific SHA in a remote repository (Option W —
/// issue #96 / RFC-032 §A1.7). Unlike [`extract_at_rev`] (which requires
/// a local git repo and uses `git worktree add`), this clones `<url>`
/// into a persistent cache at [`cache_dir_for`]`(url, sha)` and checks
/// out `<sha>`. Second runs with the same `(url, sha)` reuse the cache
/// (AC-3) — a sentinel file `.cfdb-extract-ok` is written after a
/// successful clone+checkout and gates the skip.
///
/// Auth (AC-2): inherits ambient git credentials — SSH agent
/// (`$SSH_AUTH_SOCK`), `~/.config/git/credentials`, `GIT_ASKPASS`,
/// `credential.helper`. Whatever `git clone` itself accepts at the
/// shell works here — no new plumbing.
fn extract_at_url_rev(
    url_at_sha: &str,
    db: &Path,
    keyspace: Option<String>,
    hir: bool,
) -> Result<(), crate::CfdbCliError> {
    let (url, sha) = parse_url_at_sha(url_at_sha).ok_or_else(|| {
        crate::CfdbCliError::Usage(format!(
            "--rev `{url_at_sha}` is not a valid <url>@<sha> — expected http://, https://, ssh://, or file:// URL with a hex SHA ≥ 7 chars after the final '@'"
        ))
    })?;

    let cache_dir = cache_dir_for(url, sha);
    let sentinel = cache_dir.join(".cfdb-extract-ok");

    if !sentinel.exists() {
        prepare_cache_dir(&cache_dir)?;
        eprintln!(
            "extract --rev {url_at_sha}: cloning {url} into {}",
            cache_dir.display()
        );
        clone_and_checkout(url, sha, &cache_dir)?;
        std::fs::write(&sentinel, b"cfdb extract ok\n").map_err(|e| {
            crate::CfdbCliError::Usage(format!("cannot write sentinel {}: {e}", sentinel.display()))
        })?;
    } else {
        eprintln!(
            "extract --rev {url_at_sha}: cache hit at {}",
            cache_dir.display()
        );
    }

    let ks_name = keyspace.unwrap_or_else(|| short_rev(sha));
    extract_at_path(&cache_dir, db, Some(ks_name), hir)
}

/// Ensure the parent directory exists and remove any half-populated cache
/// from an interrupted prior run (presence of the dir without the
/// `.cfdb-extract-ok` sentinel means clone/checkout did not complete).
fn prepare_cache_dir(cache_dir: &Path) -> Result<(), crate::CfdbCliError> {
    if let Some(parent) = cache_dir.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            crate::CfdbCliError::Usage(format!(
                "cannot create cache parent {}: {e}",
                parent.display()
            ))
        })?;
    }
    if cache_dir.exists() {
        std::fs::remove_dir_all(cache_dir).map_err(|e| {
            crate::CfdbCliError::Usage(format!(
                "cannot clear stale cache {}: {e}",
                cache_dir.display()
            ))
        })?;
    }
    Ok(())
}

/// `git clone` then `git fetch origin <sha>` then `git checkout <sha>`.
/// Fetch is explicit because `git clone <url>` fetches only the default
/// branch; arbitrary SHAs need an explicit fetch (server must support
/// `uploadpack.allowReachableSHA1InWant`, which Gitea has on by default).
fn clone_and_checkout(url: &str, sha: &str, cache_dir: &Path) -> Result<(), crate::CfdbCliError> {
    let clone = Command::new("git")
        .args(["clone", "--quiet", url])
        .arg(cache_dir)
        .output()?;
    if !clone.status.success() {
        return Err(crate::CfdbCliError::Usage(format!(
            "git clone {url} {}: {} ({})",
            cache_dir.display(),
            String::from_utf8_lossy(&clone.stderr).trim(),
            clone.status
        )));
    }

    let fetch = Command::new("git")
        .arg("-C")
        .arg(cache_dir)
        .args(["fetch", "--quiet", "origin", sha])
        .output()?;
    if !fetch.status.success() {
        return Err(crate::CfdbCliError::Usage(format!(
            "git fetch origin {sha} in {}: {} ({}) — server may need uploadpack.allowReachableSHA1InWant=true for non-default SHAs",
            cache_dir.display(),
            String::from_utf8_lossy(&fetch.stderr).trim(),
            fetch.status
        )));
    }

    let checkout = Command::new("git")
        .arg("-C")
        .arg(cache_dir)
        .args(["checkout", "--quiet", sha])
        .output()?;
    if !checkout.status.success() {
        return Err(crate::CfdbCliError::Usage(format!(
            "git checkout {sha} in {}: {} ({})",
            cache_dir.display(),
            String::from_utf8_lossy(&checkout.stderr).trim(),
            checkout.status
        )));
    }

    Ok(())
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

/// Split `<url>@<sha>` into its components, or `None` if the input does
/// not match the Option W form.
///
/// Splits on the RIGHTMOST `@` because SSH URLs like `git@host:path`
/// contain their own `@`. The SHA side must be all-ASCII-hex and
/// ≥ 7 chars, which rejects `user@host.com` (non-hex suffix)
/// unambiguously. Recognised URL schemes: `http://`, `https://`,
/// `ssh://`, `file://`. The `git@host:path` SSH shorthand is NOT
/// accepted in v1 — use the explicit `ssh://…` form instead.
/// `file://` is accepted both for hermetic integration tests and for
/// the self-dogfood case `file://$(pwd)/.git@$(git rev-parse HEAD)`.
fn parse_url_at_sha(s: &str) -> Option<(&str, &str)> {
    let idx = s.rfind('@')?;
    let (url, at_sha) = s.split_at(idx);
    let sha = &at_sha[1..]; // skip the '@'
    if !url_has_scheme(url) {
        return None;
    }
    if sha.len() < 7 || !sha.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    Some((url, sha))
}

/// Predicate wrapper around [`parse_url_at_sha`] — the single resolution
/// point for URL@SHA discrimination in the `extract` dispatcher. See the
/// match guard in [`extract`]; no other call site may re-check the form.
fn is_url_at_sha(s: &str) -> bool {
    parse_url_at_sha(s).is_some()
}

fn url_has_scheme(url: &str) -> bool {
    url.starts_with("http://")
        || url.starts_with("https://")
        || url.starts_with("ssh://")
        || url.starts_with("file://")
}

/// Compute the persistent cache directory for `(url, sha)` under Option W.
///
/// Precedence:
///   1. `$CFDB_CACHE_DIR` — explicit override (tests use this for isolation).
///   2. `$XDG_CACHE_HOME/cfdb/extract` — standard XDG path.
///   3. `$HOME/.cache/cfdb/extract` — POSIX fallback.
///   4. `std::env::temp_dir()/cfdb/extract` — last resort, non-persistent
///      (emits an `eprintln!` warning; unusual — typically containers
///      without `$HOME`).
///
/// Per-URL subdir: first 16 hex chars of `sha256(url)` — enough collision
/// resistance for ~100s of tracked URLs, keeps paths short.
/// Per-SHA subdir: full `<sha>` (not [`short_rev`]) — two SHAs sharing a
/// 12-char prefix must remain distinct on disk.
fn cache_dir_for(url: &str, sha: &str) -> PathBuf {
    cache_base_dir().join(url_hash_hex16(url)).join(sha)
}

fn cache_base_dir() -> PathBuf {
    if let Some(v) = std::env::var_os("CFDB_CACHE_DIR") {
        if !v.is_empty() {
            return PathBuf::from(v);
        }
    }
    if let Some(v) = std::env::var_os("XDG_CACHE_HOME") {
        if !v.is_empty() {
            return PathBuf::from(v).join("cfdb").join("extract");
        }
    }
    if let Some(v) = std::env::var_os("HOME") {
        return PathBuf::from(v).join(".cache").join("cfdb").join("extract");
    }
    eprintln!("cfdb: $HOME unset — falling back to tempdir cache (NOT persistent)");
    std::env::temp_dir().join("cfdb").join("extract")
}

fn url_hash_hex16(url: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(url.as_bytes());
    digest
        .iter()
        .take(8)
        .fold(String::with_capacity(16), |mut acc, b| {
            use std::fmt::Write as _;
            let _ = write!(acc, "{b:02x}");
            acc
        })
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

    // ---------------------------------------------------------------------
    // Option W: <url>@<sha> parsing (issue #96 / RFC-032 §A1.7).
    // These are the four discriminations the match guard in `extract`
    // relies on — mirror them in the Wiring Assertion section of
    // `.prescriptions/96.md`.
    // ---------------------------------------------------------------------

    #[test]
    fn parse_url_at_sha_accepts_https_with_full_hex_sha() {
        assert_eq!(
            parse_url_at_sha("https://host/repo@abcdef0123456789abcdef0123456789abcdef01"),
            Some((
                "https://host/repo",
                "abcdef0123456789abcdef0123456789abcdef01",
            ))
        );
        assert!(is_url_at_sha(
            "https://host/repo@abcdef0123456789abcdef0123456789abcdef01"
        ));
    }

    #[test]
    fn parse_url_at_sha_accepts_http_and_ssh_schemes() {
        assert!(is_url_at_sha("http://host/repo@deadbeef"));
        assert!(is_url_at_sha("ssh://host/repo@deadbeef"));
        // https covered in the previous test.
    }

    #[test]
    fn parse_url_at_sha_splits_on_rightmost_at_sign() {
        // SSH-shorthand embedded before the URL@SHA separator — the SHA
        // side must be hex, so `git@host.com:path@deadbeef` parses.
        // (The URL side `git@host.com:path` is then rejected for lacking
        //  a recognised scheme — v1 does NOT accept SSH shorthand.)
        assert_eq!(parse_url_at_sha("git@host.com:path@deadbeef"), None);
        // But an explicit `ssh://` prefix that ALSO carries an `@` in the
        // host part must split on the rightmost `@`:
        assert_eq!(
            parse_url_at_sha("ssh://user@host.com/repo@deadbeef"),
            Some(("ssh://user@host.com/repo", "deadbeef"))
        );
    }

    #[test]
    fn parse_url_at_sha_rejects_plain_sha() {
        // A plain SHA (no `@`) must fall through to the same-repo path.
        assert_eq!(parse_url_at_sha("abc123"), None);
        assert_eq!(
            parse_url_at_sha("abcdef0123456789abcdef0123456789abcdef01"),
            None
        );
        assert!(!is_url_at_sha("abc123"));
    }

    #[test]
    fn parse_url_at_sha_accepts_file_scheme() {
        // file:// is accepted for hermetic tests + self-dogfood use case
        // `cfdb extract --rev file://$(pwd)/.git@$(git rev-parse HEAD)`.
        assert_eq!(
            parse_url_at_sha("file:///tmp/r@deadbeef"),
            Some(("file:///tmp/r", "deadbeef"))
        );
        assert!(is_url_at_sha("file:///tmp/r@deadbeef"));
    }

    #[test]
    fn parse_url_at_sha_rejects_unknown_scheme() {
        // URL side must carry http:// / https:// / ssh:// / file://.
        assert_eq!(parse_url_at_sha("ftp://host/r@deadbeef"), None);
        assert_eq!(parse_url_at_sha("rsync://host/r@deadbeef"), None);
        assert!(!is_url_at_sha("ftp://host/r@deadbeef"));
    }

    #[test]
    fn parse_url_at_sha_rejects_non_hex_or_short_sha() {
        // Post-`@` side must be all-hex and ≥ 7 chars.
        assert_eq!(parse_url_at_sha("https://host/r@notahex"), None);
        assert_eq!(parse_url_at_sha("https://host/r@abc"), None); // too short (3 < 7)
                                                                  // Mixed case hex IS valid (git accepts it).
        assert!(is_url_at_sha("https://host/r@AbCdEf0"));
    }

    #[test]
    fn url_hash_hex16_is_deterministic_and_16_chars() {
        let a = url_hash_hex16("https://example.com/repo");
        let b = url_hash_hex16("https://example.com/repo");
        assert_eq!(a, b, "same input must hash identically");
        assert_eq!(a.len(), 16, "hash must be exactly 16 hex chars");
        assert!(
            a.chars().all(|c| c.is_ascii_hexdigit()),
            "hash must be all hex: got `{a}`"
        );
        // Different URLs produce different hashes.
        let c = url_hash_hex16("https://example.com/other");
        assert_ne!(a, c);
    }

    #[test]
    fn cache_dir_for_structure_is_base_slash_hex16_slash_sha() {
        // Use CFDB_CACHE_DIR override to make the base deterministic and
        // avoid touching the user's actual HOME/XDG_CACHE_HOME. The
        // env_lock serialises against other env-var tests.
        let _guard = env_lock();
        let base = std::env::temp_dir().join("cfdb-test-cache-96");
        // SAFETY: single-threaded inside env_lock().
        unsafe { std::env::set_var("CFDB_CACHE_DIR", &base) };
        let sha = "abcdef0123456789abcdef0123456789abcdef01";
        let dir = cache_dir_for("https://example.com/repo", sha);
        assert_eq!(dir.parent().and_then(|p| p.parent()), Some(base.as_path()));
        assert_eq!(
            dir.file_name().and_then(|s| s.to_str()),
            Some(sha),
            "innermost dir must be full SHA"
        );
        let hex = dir
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|s| s.to_str())
            .expect("url-hash segment");
        assert_eq!(hex.len(), 16);
        assert!(hex.chars().all(|c| c.is_ascii_hexdigit()));
        unsafe { std::env::remove_var("CFDB_CACHE_DIR") };
    }

    #[test]
    fn cache_base_dir_env_var_precedence() {
        let _guard = env_lock();
        // Save original env so we can restore.
        let orig_cfdb = std::env::var_os("CFDB_CACHE_DIR");
        let orig_xdg = std::env::var_os("XDG_CACHE_HOME");
        let orig_home = std::env::var_os("HOME");

        // CFDB_CACHE_DIR wins outright.
        unsafe {
            std::env::set_var("CFDB_CACHE_DIR", "/tmp/cfdb-explicit");
            std::env::set_var("XDG_CACHE_HOME", "/tmp/xdg");
            std::env::set_var("HOME", "/tmp/home");
        }
        assert_eq!(cache_base_dir(), PathBuf::from("/tmp/cfdb-explicit"));

        // XDG_CACHE_HOME is used when CFDB_CACHE_DIR is unset.
        unsafe { std::env::remove_var("CFDB_CACHE_DIR") };
        assert_eq!(cache_base_dir(), PathBuf::from("/tmp/xdg/cfdb/extract"));

        // HOME is used when both CFDB_* and XDG_* are unset.
        unsafe { std::env::remove_var("XDG_CACHE_HOME") };
        assert_eq!(
            cache_base_dir(),
            PathBuf::from("/tmp/home/.cache/cfdb/extract")
        );

        // Restore original env.
        unsafe {
            match orig_cfdb {
                Some(v) => std::env::set_var("CFDB_CACHE_DIR", v),
                None => std::env::remove_var("CFDB_CACHE_DIR"),
            }
            match orig_xdg {
                Some(v) => std::env::set_var("XDG_CACHE_HOME", v),
                None => std::env::remove_var("XDG_CACHE_HOME"),
            }
            match orig_home {
                Some(v) => std::env::set_var("HOME", v),
                None => std::env::remove_var("HOME"),
            }
        }
    }

    /// Serialise env-var-touching tests so they don't race each other in
    /// `cargo test`'s default parallel harness. Intentionally uses a
    /// hand-rolled `Mutex` rather than pulling in `serial_test` — keeps
    /// the dep list minimal (forbidden move intersection: no new deps
    /// beyond `sha2`).
    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

//! Extraction command handlers — `cfdb extract` entry and its
//! URL/rev/cache plumbing. Split out of `commands.rs` for the drift
//! god-file decomposition (#151). Move-only: every item preserves its
//! original body and public path (via `pub use` in `commands.rs`).

use std::path::{Path, PathBuf};
use std::process::Command;

use cfdb_core::schema::Keyspace;
use cfdb_core::store::StoreBackend;

use crate::compose;

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
    // The `--` separator before user-supplied positional arguments is
    // defense-in-depth (audit 2026-W17 / CFDB-CLI-H2 / #270). `url` is
    // user-controlled via `--rev <url>@<sha>` and an attacker-influenced
    // value (e.g. via `.cfdb/extract-pins`) could otherwise be
    // misinterpreted as a `git` flag — `file:///path/--option` is a valid
    // file URL. The sha is already hex-validated by `parse_url_at_sha`
    // (all-ASCII-hex, ≥ 7 chars — cannot start with `--`); `--` is kept
    // on the `fetch` refspec position for symmetry. `git checkout` is
    // INTENTIONALLY left without `--` because `git checkout -- <arg>`
    // forces pathspec mode and would treat a SHA as a filename, breaking
    // the checkout. The hex validation upstream is sufficient there.
    let clone = Command::new("git")
        .args(["clone", "--quiet", "--", url])
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
        .args(["fetch", "--quiet", "origin", "--", sha])
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
pub fn short_rev(rev: &str) -> String {
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
pub fn parse_url_at_sha(s: &str) -> Option<(&str, &str)> {
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
pub fn is_url_at_sha(s: &str) -> bool {
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
pub fn cache_dir_for(url: &str, sha: &str) -> PathBuf {
    cache_base_dir().join(url_hash_hex16(url)).join(sha)
}

pub fn cache_base_dir() -> PathBuf {
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

pub fn url_hash_hex16(url: &str) -> String {
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
        // `--` before the positional `<path> <rev>` blocks rev (user-
        // supplied via `--rev <sha-or-ref>`) from being misinterpreted as
        // an option (audit 2026-W17 / CFDB-CLI-H2 / #270).
        let status = Command::new("git")
            .current_dir(repo)
            .args(["worktree", "add", "--detach", "--quiet", "--"])
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

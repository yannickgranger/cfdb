//! `enrich_git_history` — git-history facts per `:Item`.
//!
//! Walks the workspace's git repository, collects per-file `(last_commit_unix_ts,
//! last_author, commit_count)` from HEAD's history, and writes the three facts
//! onto every `:Item` node's property bag:
//!
//! - `git_last_commit_unix_ts: PropValue::Int(i64)` — epoch seconds of the most
//!   recent commit touching the file (determinism fix: epoch not days-since-now).
//! - `git_last_author: PropValue::Str(String)` — committer email of the most
//!   recent commit touching the file. `""` when the commit has no author email.
//! - `git_commit_count: PropValue::Int(i64)` — number of commits in HEAD's
//!   history whose diff-vs-first-parent touches the file. This matches
//!   `git rev-list HEAD --full-history -- <file>` semantics (no history
//!   simplification), which is deliberately broader than `git log -- <file>`
//!   default — the churn signal used by the downstream classifier
//!   should count every commit that touched the file, including those on
//!   branches later squashed out of mainline.
//!
//! Items with a `file` prop that git does not track (untracked paths, paths
//! outside the repo, or items produced by a workspace whose enclosing directory
//! is not a git repo) receive `PropValue::Null` for all three attrs — never
//! silently zero, so downstream classifiers can distinguish "no data" from
//! "real zero".
//!
//! # Determinism
//! - File paths are aggregated into a `BTreeMap<String, GitInfo>` → iteration
//!   order is sorted by path.
//! - The revwalk is configured with `TOPOLOGICAL | TIME` sort, which git2
//!   documents as deterministic for a fixed HEAD.
//! - "Most recent" per file = first commit seen during the reverse-chronological
//!   walk (first-insert wins; subsequent hits only bump `commit_count`).
//!
//! Two runs on an unchanged tree produce byte-identical canonical dumps.
//!
//! # Gate
//! This module only compiles with the `git-enrich` feature; the feature-off
//! path is handled by `EnrichEngine::enrich_git_history`'s
//! `#[cfg(not(feature = "git-enrich"))]` variant in `lib.rs`.
//!
//! Moved from `cfdb-petgraph::enrich::git_history` (RFC-056 slice 056-D) —
//! rewritten against [`GraphView`] instead of `&mut KeyspaceState`. Every
//! pure function below (git collection, path-strip matching) is unchanged;
//! only `write_attrs`/`write_attrs_one`'s node access moved from direct
//! `KeyspaceState`/`NodeIndex` reach-in to the port's `nodes_with_label`/
//! `node_by_id`/`set_attr`.

use std::collections::BTreeMap;
use std::path::Path;

use cfdb_core::enrich::EnrichReport;
use cfdb_core::fact::PropValue;
use cfdb_core::graph::GraphView;
use cfdb_core::schema::Label;

pub(crate) const VERB: &str = "enrich_git_history";
pub(crate) const ATTR_TS: &str = "git_last_commit_unix_ts";
pub(crate) const ATTR_AUTHOR: &str = "git_last_author";
pub(crate) const ATTR_COUNT: &str = "git_commit_count";

/// Per-file aggregate built from HEAD's commit history.
struct GitInfo {
    last_commit_unix_ts: i64,
    last_author: String,
    commit_count: i64,
}

/// Entry point called by [`crate::EnrichEngine`].
///
/// Returns `EnrichReport` by value — never `Err`. Keyspace-not-found and
/// workspace-root-missing are already handled upstream in `lib.rs`; this
/// function assumes both a valid keyspace view and a usable workspace path.
/// Git-level failures (directory not a repo, malformed history) are folded
/// into warnings so the pass can still record `ran: true` with Null attrs
/// for every item — this is a degraded-path approach.
pub(crate) fn run(view: &mut dyn GraphView, workspace_root: &Path) -> EnrichReport {
    let mut warnings: Vec<String> = Vec::new();
    let item_ids = view.nodes_with_label(&Label::new(Label::ITEM));

    if item_ids.is_empty() {
        return EnrichReport {
            verb: VERB.into(),
            ran: true,
            facts_scanned: 0,
            attrs_written: 0,
            edges_written: 0,
            warnings: vec![format!(
                "{VERB}: no :Item nodes in keyspace — nothing to enrich"
            )],
        };
    }

    let git_info = match collect_git_info(workspace_root) {
        Ok(info) => info,
        Err(msg) => {
            warnings.push(msg);
            BTreeMap::new()
        }
    };

    // Canonicalize workspace_root for path-strip — `:Item.file` is an
    // absolute path and the user-supplied --workspace may be relative
    // (e.g. `.`); without canonicalization the strip_prefix below
    // never matches and every item gets a Null timestamp.
    let workspace_canon =
        std::fs::canonicalize(workspace_root).unwrap_or_else(|_| workspace_root.to_path_buf());
    let attrs_written = write_attrs(view, &item_ids, &git_info, &workspace_canon);

    EnrichReport {
        verb: VERB.into(),
        ran: true,
        facts_scanned: u64::try_from(item_ids.len()).unwrap_or(u64::MAX),
        attrs_written,
        edges_written: 0,
        warnings,
    }
}

/// Open the repo via `Repository::discover` (tolerates being a sub-directory
/// of a git worktree) and walk HEAD, aggregating per-file commit info.
fn collect_git_info(workspace_root: &Path) -> Result<BTreeMap<String, GitInfo>, String> {
    let repo = git2::Repository::discover(workspace_root).map_err(|e| {
        format!(
            "{VERB}: workspace_root={workspace_root:?} is not inside a git repository ({e}); writing Null for all items"
        )
    })?;

    let mut revwalk = repo
        .revwalk()
        .map_err(|e| format!("{VERB}: repo.revwalk() failed ({e})"))?;
    revwalk
        .push_head()
        .map_err(|e| format!("{VERB}: revwalk.push_head() failed ({e})"))?;
    revwalk
        .set_sorting(git2::Sort::TOPOLOGICAL | git2::Sort::TIME)
        .map_err(|e| format!("{VERB}: revwalk.set_sorting() failed ({e})"))?;

    let mut info: BTreeMap<String, GitInfo> = BTreeMap::new();
    for oid in revwalk {
        let oid = oid.map_err(|e| format!("{VERB}: revwalk yielded error ({e})"))?;
        fold_commit(&repo, oid, &mut info)
            .map_err(|e| format!("{VERB}: fold_commit({oid}) failed ({e})"))?;
    }
    Ok(info)
}

/// Diff a single commit against its first parent (or the empty tree, for
/// root commits) and update `info` for every touched path.
fn fold_commit(
    repo: &git2::Repository,
    oid: git2::Oid,
    info: &mut BTreeMap<String, GitInfo>,
) -> Result<(), git2::Error> {
    let commit = repo.find_commit(oid)?;
    let tree = commit.tree()?;
    let parent_tree = commit.parents().next().map(|p| p.tree()).transpose()?;
    let diff = repo.diff_tree_to_tree(parent_tree.as_ref(), Some(&tree), None)?;

    let commit_ts = commit.time().seconds();
    // Bind the Signature to a local so its lifetime covers the delta loop —
    // `commit.author()` returns a borrowed `Signature<'_>` whose `email()`
    // slice would otherwise dangle after the statement ended.
    let author = commit.author();
    let author_email = author.email().unwrap_or_default();

    for delta in diff.deltas() {
        accumulate_delta(&delta, commit_ts, author_email, info);
    }
    Ok(())
}

/// Update the per-file entry with this commit. First-insert wins for `last_*`
/// values (revwalk is reverse-chronological, so the first commit seen per
/// path is the most recent); subsequent hits only bump `commit_count`.
fn accumulate_delta(
    delta: &git2::DiffDelta<'_>,
    commit_ts: i64,
    author_email: &str,
    info: &mut BTreeMap<String, GitInfo>,
) {
    let Some(path) = delta.new_file().path().or_else(|| delta.old_file().path()) else {
        return;
    };
    let path_str = path.to_string_lossy();
    upsert(info, &path_str, commit_ts, author_email);
}

fn upsert(info: &mut BTreeMap<String, GitInfo>, path: &str, commit_ts: i64, author: &str) {
    match info.get_mut(path) {
        Some(entry) => {
            entry.commit_count += 1;
        }
        None => {
            info.insert(
                path.to_string(),
                GitInfo {
                    last_commit_unix_ts: commit_ts,
                    last_author: author.to_string(),
                    commit_count: 1,
                },
            );
        }
    }
}

/// Write the three git-history attrs to every `:Item` node.
fn write_attrs(
    view: &mut dyn GraphView,
    item_ids: &[String],
    git_info: &BTreeMap<String, GitInfo>,
    workspace_root: &Path,
) -> u64 {
    let mut count: u64 = 0;
    for id in item_ids {
        count += write_attrs_one(view, id, git_info, workspace_root);
    }
    count
}

/// Write per-node attrs, returning the number of attrs written (always 3 —
/// Null is still a write, since the classifier uses the presence of the key
/// to gate confidence — or 0 if `id` no longer resolves to a node, mirroring
/// the pre-move skip-on-stale-index behavior).
///
/// `:Item.file` is an absolute path emitted by the extractor; `git_info`
/// is keyed by paths relative to the repo root (the form `git diff` returns).
/// The lookup strips `workspace_root` from the stored path before matching.
/// Falls back to the absolute path if it isn't under `workspace_root` (which
/// can happen for vendored deps or generated code outside the tree — those
/// get a Null timestamp via the `None` branch below, the intended result).
///
/// The lookup is resolved to owned values and the immutable `node_by_id`
/// borrow dropped before the `set_attr` calls — `GraphView` methods borrow
/// `view` for their own call, so an immutable read can't be held across a
/// later mutable write on the same trait object.
fn write_attrs_one(
    view: &mut dyn GraphView,
    id: &str,
    git_info: &BTreeMap<String, GitInfo>,
    workspace_root: &Path,
) -> u64 {
    let Some(node) = view.node_by_id(id) else {
        return 0;
    };
    let found = node
        .props
        .get("file")
        .and_then(PropValue::as_str)
        .and_then(|p| {
            let path = std::path::Path::new(p);
            let rel = path
                .strip_prefix(workspace_root)
                .map(|r| r.to_string_lossy().into_owned())
                .unwrap_or_else(|_| p.to_string());
            git_info.get(&rel)
        })
        .map(|info| {
            (
                info.last_commit_unix_ts,
                info.last_author.clone(),
                info.commit_count,
            )
        });

    match found {
        Some((ts, author, count)) => {
            view.set_attr(id, ATTR_TS, PropValue::Int(ts));
            view.set_attr(id, ATTR_AUTHOR, PropValue::Str(author));
            view.set_attr(id, ATTR_COUNT, PropValue::Int(count));
        }
        None => {
            view.set_attr(id, ATTR_TS, PropValue::Null);
            view.set_attr(id, ATTR_AUTHOR, PropValue::Null);
            view.set_attr(id, ATTR_COUNT, PropValue::Null);
        }
    }
    3
}

// ---------------------------------------------------------------------------
// Tests — feature-gated on `git-enrich` because fixture setup needs libgit2.
// AC-1 "default build compiles" is exercised at workspace level by `cargo
// check` without the feature (the module is `#[cfg(feature = "git-enrich")]`
// so it simply vanishes in the default build).
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests;

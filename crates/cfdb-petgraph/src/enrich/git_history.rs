//! `enrich_git_history` — git-history facts per `:Item` (slice 43-B / issue #105).
//!
//! Walks the workspace's git repository, collects per-file `(last_commit_unix_ts,
//! last_author, commit_count)` from HEAD's history, and writes the three facts
//! onto every `:Item` node's property bag:
//!
//! - `git_last_commit_unix_ts: PropValue::Int(i64)` — epoch seconds of the most
//!   recent commit touching the file (RFC addendum §A2.2 row 1; clean-arch B2
//!   determinism fix: epoch not days-since-now).
//! - `git_last_author: PropValue::Str(String)` — committer email of the most
//!   recent commit touching the file. `""` when the commit has no author email.
//! - `git_commit_count: PropValue::Int(i64)` — number of commits in HEAD's
//!   history whose diff-vs-first-parent touches the file. This matches
//!   `git rev-list HEAD --full-history -- <file>` semantics (no history
//!   simplification), which is deliberately broader than `git log -- <file>`
//!   default — the churn signal used by the downstream classifier
//!   (`docs/RFC-cfdb-v0.2-addendum-draft.md` §A2.1 class 5 / §A2.2 row 1)
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
//!
//! - File paths are aggregated into a `BTreeMap<String, GitInfo>` → iteration
//!   order is sorted by path.
//! - The revwalk is configured with `TOPOLOGICAL | TIME` sort, which git2
//!   documents as deterministic for a fixed HEAD.
//! - "Most recent" per file = first commit seen during the reverse-chronological
//!   walk (first-insert wins; subsequent hits only bump `commit_count`).
//!
//! Two runs on an unchanged tree produce byte-identical canonical dumps (AC-6).
//!
//! # Gate
//!
//! This module only compiles with the `git-enrich` feature. The feature-off
//! path is handled in `crate::enrich_git_history_feature_off`.

use std::collections::BTreeMap;
use std::path::Path;

use cfdb_core::enrich::EnrichReport;
use cfdb_core::fact::{Node, PropValue};
use cfdb_core::schema::Label;

use crate::graph::KeyspaceState;

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

/// Entry point called by `impl EnrichBackend for PetgraphStore` in `crate`.
///
/// Returns `EnrichReport` by value — never `Err`. Keyspace-not-found and
/// workspace-root-missing are already handled upstream in `lib.rs`; this
/// function assumes both a valid keyspace state and a usable workspace path.
/// Git-level failures (directory not a repo, malformed history) are folded
/// into warnings so the pass can still record `ran: true` with Null attrs
/// for every item — clean-arch B3 degraded-path analogue.
pub(crate) fn run(state: &mut KeyspaceState, workspace_root: &Path) -> EnrichReport {
    let mut warnings: Vec<String> = Vec::new();
    let item_indices = state.nodes_with_label(&Label::new(Label::ITEM));

    if item_indices.is_empty() {
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

    let attrs_written = write_attrs(state, &item_indices, &git_info);

    EnrichReport {
        verb: VERB.into(),
        ran: true,
        facts_scanned: u64::try_from(item_indices.len()).unwrap_or(u64::MAX),
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
    state: &mut KeyspaceState,
    item_indices: &[petgraph::stable_graph::NodeIndex],
    git_info: &BTreeMap<String, GitInfo>,
) -> u64 {
    let mut count: u64 = 0;
    for &idx in item_indices {
        let Some(node) = state.graph.node_weight_mut(idx) else {
            continue;
        };
        count += write_attrs_one(node, git_info);
    }
    count
}

/// Write per-node attrs, returning the number of attrs written (always 3 —
/// Null is still a write, since the classifier uses the presence of the key
/// to gate confidence).
fn write_attrs_one(node: &mut Node, git_info: &BTreeMap<String, GitInfo>) -> u64 {
    let lookup = node
        .props
        .get("file")
        .and_then(PropValue::as_str)
        .and_then(|p| git_info.get(p));

    match lookup {
        Some(info) => {
            node.props
                .insert(ATTR_TS.into(), PropValue::Int(info.last_commit_unix_ts));
            node.props
                .insert(ATTR_AUTHOR.into(), PropValue::Str(info.last_author.clone()));
            node.props
                .insert(ATTR_COUNT.into(), PropValue::Int(info.commit_count));
        }
        None => {
            node.props.insert(ATTR_TS.into(), PropValue::Null);
            node.props.insert(ATTR_AUTHOR.into(), PropValue::Null);
            node.props.insert(ATTR_COUNT.into(), PropValue::Null);
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
mod tests {
    use std::path::Path;

    use cfdb_core::enrich::EnrichBackend;
    use cfdb_core::fact::{Node, PropValue, Props};
    use cfdb_core::schema::{Keyspace, Label};
    use cfdb_core::store::StoreBackend;

    use crate::PetgraphStore;

    // ------------------------------------------------------------------
    // Fixture builders — a tempdir + a fresh git repo + one or more files
    // committed along a linear history.
    // ------------------------------------------------------------------

    struct GitFixture {
        _tmp: tempfile::TempDir,
        workspace: std::path::PathBuf,
        repo: git2::Repository,
    }

    impl GitFixture {
        fn new() -> Self {
            let tmp = tempfile::tempdir().expect("tempdir");
            let workspace = tmp.path().to_path_buf();
            let repo = git2::Repository::init(&workspace).expect("git init");
            let mut cfg = repo.config().expect("repo.config");
            cfg.set_str("user.name", "Test Author").expect("cfg name");
            cfg.set_str("user.email", "test@example.com")
                .expect("cfg email");
            GitFixture {
                _tmp: tmp,
                workspace,
                repo,
            }
        }

        fn write(&self, rel: &str, contents: &str) {
            let path = self.workspace.join(rel);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).expect("mkdirs");
            }
            std::fs::write(&path, contents).expect("write file");
        }

        fn commit(&self, rel: &str, message: &str, time: i64) -> git2::Oid {
            let mut index = self.repo.index().expect("index");
            index.add_path(Path::new(rel)).expect("add_path");
            index.write().expect("index.write");
            let tree_oid = index.write_tree().expect("write_tree");
            let tree = self.repo.find_tree(tree_oid).expect("find_tree");
            let parents: Vec<git2::Commit<'_>> = match self.repo.head() {
                Ok(head) => vec![head.peel_to_commit().expect("peel_to_commit")],
                Err(_) => Vec::new(),
            };
            let parent_refs: Vec<&git2::Commit<'_>> = parents.iter().collect();
            let sig =
                git2::Signature::new("Test Author", "test@example.com", &git2::Time::new(time, 0))
                    .expect("sig");
            self.repo
                .commit(Some("HEAD"), &sig, &sig, message, &tree, &parent_refs)
                .expect("commit")
        }
    }

    fn store_with_item(workspace: &Path, file_path: &str, item_qname: &str) -> PetgraphStore {
        let mut store = PetgraphStore::new().with_workspace(workspace);
        let ks = Keyspace::new("test");
        let mut props = Props::new();
        props.insert("qname".into(), PropValue::Str(item_qname.to_string()));
        props.insert("file".into(), PropValue::Str(file_path.to_string()));
        let node = Node {
            id: format!("item:{item_qname}"),
            label: Label::new(Label::ITEM),
            props,
        };
        store.ingest_nodes(&ks, vec![node]).expect("ingest_nodes");
        store
    }

    fn get_item_props(store: &PetgraphStore, keyspace: &Keyspace, qname: &str) -> Props {
        let (nodes, _edges) = store.export(keyspace).expect("export");
        nodes
            .into_iter()
            .find(|n| {
                n.props
                    .get("qname")
                    .and_then(PropValue::as_str)
                    .is_some_and(|q| q == qname)
            })
            .unwrap_or_else(|| panic!("item {qname} not found"))
            .props
    }

    // ------------------------------------------------------------------
    // AC-2: two-commit fixture — counts + last-ts + last-author correct.
    // ------------------------------------------------------------------

    #[test]
    fn ac2_two_commit_fixture_writes_correct_attrs() {
        let fx = GitFixture::new();
        fx.write("src/lib.rs", "fn v1() {}\n");
        fx.commit("src/lib.rs", "first", 1_700_000_000);
        fx.write("src/lib.rs", "fn v2() {}\n");
        fx.commit("src/lib.rs", "second", 1_700_000_100);

        let mut store = store_with_item(&fx.workspace, "src/lib.rs", "crate::v2");
        let ks = Keyspace::new("test");
        let report = store.enrich_git_history(&ks).expect("pass");

        assert!(report.ran, "pass should run: {:?}", report.warnings);
        assert_eq!(report.attrs_written, 3, "one :Item × three attrs");

        let props = get_item_props(&store, &ks, "crate::v2");
        assert_eq!(
            props.get(super::ATTR_TS),
            Some(&PropValue::Int(1_700_000_100)),
            "most recent commit timestamp"
        );
        assert_eq!(
            props.get(super::ATTR_AUTHOR),
            Some(&PropValue::Str("test@example.com".into())),
            "committer email"
        );
        assert_eq!(
            props.get(super::ATTR_COUNT),
            Some(&PropValue::Int(2)),
            "two commits touched src/lib.rs"
        );
    }

    // ------------------------------------------------------------------
    // AC-3: untracked-file fixture — attrs all Null, no panic.
    // ------------------------------------------------------------------

    #[test]
    fn ac3_untracked_file_gets_null_attrs() {
        let fx = GitFixture::new();
        fx.write("src/tracked.rs", "fn tracked() {}\n");
        fx.commit("src/tracked.rs", "initial", 1_700_000_000);
        fx.write("src/untracked.rs", "fn untracked() {}\n");

        let mut store = store_with_item(&fx.workspace, "src/untracked.rs", "crate::untracked");
        let ks = Keyspace::new("test");
        let report = store.enrich_git_history(&ks).expect("pass");

        assert!(report.ran);
        let props = get_item_props(&store, &ks, "crate::untracked");
        assert_eq!(props.get(super::ATTR_TS), Some(&PropValue::Null));
        assert_eq!(props.get(super::ATTR_AUTHOR), Some(&PropValue::Null));
        assert_eq!(props.get(super::ATTR_COUNT), Some(&PropValue::Null));
    }

    // ------------------------------------------------------------------
    // AC-6: determinism — two runs produce identical canonical dumps.
    // ------------------------------------------------------------------

    #[test]
    fn ac6_two_runs_produce_identical_canonical_dumps() {
        let fx = GitFixture::new();
        fx.write("src/a.rs", "a\n");
        fx.commit("src/a.rs", "a1", 1_700_000_000);
        fx.write("src/b.rs", "b\n");
        fx.commit("src/b.rs", "b1", 1_700_000_100);
        fx.write("src/a.rs", "a2\n");
        fx.commit("src/a.rs", "a2", 1_700_000_200);

        let mut store1 = store_with_item(&fx.workspace, "src/a.rs", "crate::a");
        let mut store2 = store_with_item(&fx.workspace, "src/a.rs", "crate::a");
        let ks = Keyspace::new("test");

        store1.enrich_git_history(&ks).expect("run 1");
        store2.enrich_git_history(&ks).expect("run 2");

        let dump1 = store1.canonical_dump(&ks).expect("dump 1");
        let dump2 = store2.canonical_dump(&ks).expect("dump 2");
        assert_eq!(dump1, dump2, "two runs must be byte-identical (G1)");
    }

    // ------------------------------------------------------------------
    // Degraded paths: workspace not in a git repo.
    // ------------------------------------------------------------------

    #[test]
    fn workspace_not_a_git_repo_writes_nulls_with_warning() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let mut store = store_with_item(tmp.path(), "src/lib.rs", "crate::x");
        let ks = Keyspace::new("test");
        let report = store.enrich_git_history(&ks).expect("pass");

        assert!(report.ran, "still ran — not an error, just degraded");
        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.contains("not inside a git repository")),
            "warning must name the repo issue: {:?}",
            report.warnings
        );
        let props = get_item_props(&store, &ks, "crate::x");
        assert_eq!(props.get(super::ATTR_TS), Some(&PropValue::Null));
    }

    #[test]
    fn empty_keyspace_returns_ran_true_with_zero_counters() {
        let fx = GitFixture::new();
        fx.write("src/lib.rs", "fn _x() {}\n");
        fx.commit("src/lib.rs", "initial", 1_700_000_000);

        let mut store = PetgraphStore::new().with_workspace(&fx.workspace);
        let ks = Keyspace::new("test");
        store.ingest_nodes(&ks, Vec::new()).expect("ingest empty");
        let report = store.enrich_git_history(&ks).expect("pass");

        assert!(report.ran);
        assert_eq!(report.attrs_written, 0);
        assert_eq!(report.facts_scanned, 0);
    }

    #[test]
    fn unknown_keyspace_returns_err() {
        let fx = GitFixture::new();
        let mut store = PetgraphStore::new().with_workspace(&fx.workspace);
        let ks = Keyspace::new("never_ingested");
        let err = store
            .enrich_git_history(&ks)
            .expect_err("unknown keyspace must error");
        let msg = format!("{err:?}");
        assert!(msg.contains("UnknownKeyspace"), "{msg}");
    }

    #[test]
    fn no_workspace_root_returns_degraded_report() {
        let mut store = PetgraphStore::new();
        let ks = Keyspace::new("test");
        let mut props = Props::new();
        props.insert("qname".into(), PropValue::Str("crate::y".into()));
        props.insert("file".into(), PropValue::Str("src/lib.rs".into()));
        let node = Node {
            id: "item:crate::y".into(),
            label: Label::new(Label::ITEM),
            props,
        };
        store.ingest_nodes(&ks, vec![node]).expect("ingest");
        let report = store.enrich_git_history(&ks).expect("pass");

        assert!(!report.ran, "no workspace_root → ran=false");
        assert!(
            report.warnings.iter().any(|w| w.contains("workspace_root")),
            "warning must name the missing root: {:?}",
            report.warnings
        );
    }
}

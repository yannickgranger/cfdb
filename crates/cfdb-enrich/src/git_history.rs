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

struct GitInfo {
    last_commit_unix_ts: i64,
    last_author: String,
    commit_count: i64,
}

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
    let author = commit.author();
    let author_email = author.email().unwrap_or_default();

    for delta in diff.deltas() {
        accumulate_delta(&delta, commit_ts, author_email, info);
    }
    Ok(())
}

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

#[cfg(test)]
mod tests;

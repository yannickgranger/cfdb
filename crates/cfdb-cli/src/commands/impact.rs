use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use cfdb_core::store::QueryBackend;
use cfdb_petgraph::PetgraphStore;
use cfdb_query::{impact_query, items_with_files_query};

use crate::compose;
use crate::output;

pub fn impact(
    db: PathBuf,
    keyspace: String,
    item: Vec<String>,
    since: Option<String>,
    workspace: PathBuf,
    max_depth: Option<u32>,
) -> Result<(), crate::CfdbCliError> {
    if item.is_empty() && since.is_none() {
        return Err(
            "`cfdb impact` requires at least one --item <qname> or --since <ref>"
                .to_string()
                .into(),
        );
    }

    let (store, ks) = compose::load_store(&db, &keyspace)?;

    let seeds = if !item.is_empty() {
        item
    } else {
        let reference = since.as_deref().unwrap_or_default();
        let files = git_changed_files(&workspace, reference)?;
        resolve_seeds_from_files(&store, &ks, &files)?
    };

    if seeds.is_empty() {
        eprintln!(
            "impact: warning — no seed items resolved; blast radius is empty (a docs-only or \
             non-code change is a correct empty answer)"
        );
    }

    let query = impact_query(&seeds, max_depth);
    let result = compose::query_engine(&store).execute(&ks, &query)?;
    output::emit_json(&result)
}

fn git_changed_files(
    workspace: &Path,
    reference: &str,
) -> Result<Vec<String>, crate::CfdbCliError> {
    let range = format!("{reference}..HEAD");
    let output = Command::new("git")
        .arg("-C")
        .arg(workspace)
        .args(["diff", "--name-only"])
        .arg(&range)
        .output()
        .map_err(|e| format!("failed to run `git diff`: {e}"))?;

    if !output.status.success() {
        return Err(format!(
            "`git diff --name-only {range}` failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )
        .into());
    }

    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect())
}

fn resolve_seeds_from_files(
    store: &PetgraphStore,
    ks: &cfdb_core::schema::Keyspace,
    changed_files: &[String],
) -> Result<Vec<String>, crate::CfdbCliError> {
    if changed_files.is_empty() {
        return Ok(Vec::new());
    }
    let suffixes: Vec<String> = changed_files.iter().map(|cf| format!("/{cf}")).collect();
    let result = compose::query_engine(store).execute(ks, &items_with_files_query())?;
    let mut seeds = BTreeSet::new();
    for row in &result.rows {
        let Some(file) = row.get("file").and_then(|v| v.as_str()) else {
            continue;
        };
        if item_in_changed_set(file, changed_files, &suffixes) {
            if let Some(qname) = row.get("qname").and_then(|v| v.as_str()) {
                seeds.insert(qname.to_string());
            }
        }
    }
    Ok(seeds.into_iter().collect())
}

fn item_in_changed_set(item_file: &str, exact: &[String], suffixes: &[String]) -> bool {
    exact.iter().any(|cf| item_file == cf) || suffixes.iter().any(|s| item_file.ends_with(s))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn item_in_changed_set_matches_absolute_and_relative_paths() {
        let changed = vec!["crates/cfdb-query/src/impact.rs".to_string()];
        let suffixes = vec!["/crates/cfdb-query/src/impact.rs".to_string()];
        assert!(item_in_changed_set(
            "crates/cfdb-query/src/impact.rs",
            &changed,
            &suffixes
        ));
        assert!(item_in_changed_set(
            "/var/mnt/ws/crates/cfdb-query/src/impact.rs",
            &changed,
            &suffixes
        ));
        assert!(!item_in_changed_set(
            "crates/cfdb-query/src/lib.rs",
            &changed,
            &suffixes
        ));
        assert!(!item_in_changed_set(
            "crates/x/foobar.rs",
            &["bar.rs".to_string()],
            &["/bar.rs".to_string()]
        ));
        assert!(item_in_changed_set(
            "crates/x/bar.rs",
            &["bar.rs".to_string()],
            &["/bar.rs".to_string()]
        ));
        assert!(!item_in_changed_set("crates/x/y.rs", &[], &[]));
    }
}

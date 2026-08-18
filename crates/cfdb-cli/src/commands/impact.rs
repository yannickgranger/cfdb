//! `cfdb impact` — blast-radius dispatch.
//!
//! Resolves a set of changed-item **seeds** — either explicit `--item <qname>`
//! or `--since <ref>` (the items defined in the files `git diff --name-only
//! <ref>..HEAD` reports) — then runs the canonical reverse-reachability query
//! ([`cfdb_query::impact_query`]) to return every transitive caller: the blast
//! radius of the change. Read-only: it only issues `query`, never mutates
//! the graph.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use cfdb_core::store::QueryBackend;
use cfdb_petgraph::PetgraphStore;
use cfdb_query::{impact_query, items_with_files_query};

use crate::compose;
use crate::output;

/// `cfdb impact --db <path> --keyspace <name> [--item <qname>]... [--since
/// <ref>] [--workspace <path>]`.
///
/// Seeds come from `--item` (repeatable, exact qnames) OR `--since <ref>`
/// (file-granular: items in the changed files). At least one is required. A
/// seed set that resolves to zero items is a **Warning**, not an error — a
/// docs-only or non-code change has an empty blast radius, which is a correct
/// answer. `max_depth` (`--max-depth`) bounds the reverse traversal to N hops
/// (`None` = unbounded).
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
        // `since.is_none()` is excluded above.
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

    // `impact_query(&[], _)` is a valid query that matches nothing, so the
    // empty case flows through the same path and emits an empty result set.
    let query = impact_query(&seeds, max_depth);
    let result = compose::query_engine(&store).execute(&ks, &query)?;
    output::emit_json(&result)
}

/// `git diff --name-only <ref>..HEAD` in `workspace` → the changed file paths
/// (repo-relative, matching the `:Item.file` attribute). A non-zero git exit
/// is surfaced as an error (e.g. an unknown ref).
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

/// Resolve the seed qnames defined in `changed_files` (RFC-047 §3.3) by
/// projecting `(qname, file)` for every `:Item` and matching each item's file
/// against the changed set with [`item_in_changed_set`]. Returns a sorted,
/// de-duplicated seed list.
fn resolve_seeds_from_files(
    store: &PetgraphStore,
    ks: &cfdb_core::schema::Keyspace,
    changed_files: &[String],
) -> Result<Vec<String>, crate::CfdbCliError> {
    if changed_files.is_empty() {
        return Ok(Vec::new());
    }
    // Precompute each changed file's `/<file>` suffix ONCE, not once per
    // scanned item — the scan is over every `:Item` (40k+ on cfdb, 300k+ on
    // qbot), so a per-item `format!` would allocate millions of times.
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

/// Whether `item_file` refers to one of the changed files. `exact` are the
/// repo-relative `git diff` paths; `suffixes` is their precomputed `/<file>`
/// form. `item_file` may be absolute (HIR-extracted keyspace) or repo-relative
/// (syn-extracted), so it counts as changed when it equals an exact path OR
/// ends with a `/<file>` suffix. The leading `/` keeps `foo/bar.rs` from
/// matching a change to `bar.rs`.
fn item_in_changed_set(item_file: &str, exact: &[String], suffixes: &[String]) -> bool {
    exact.iter().any(|cf| item_file == cf) || suffixes.iter().any(|s| item_file.ends_with(s))
}

#[cfg(test)]
mod tests {
    use super::*;

    // The full `--since` seed resolution (`resolve_seeds_from_files`, which
    // constructs a `PetgraphStore`) is exercised end-to-end against a persisted
    // fixture keyspace in `tests/impact_cli.rs::impact_since_resolves_seeds_from_git_diff`
    // — store construction stays out of `src/` per the composition-root
    // invariant (RFC-044 §3.3). This module unit-tests the pure matching only.

    #[test]
    fn item_in_changed_set_matches_absolute_and_relative_paths() {
        let changed = vec!["crates/cfdb-query/src/impact.rs".to_string()];
        let suffixes = vec!["/crates/cfdb-query/src/impact.rs".to_string()];
        // repo-relative item file (syn keyspace)
        assert!(item_in_changed_set(
            "crates/cfdb-query/src/impact.rs",
            &changed,
            &suffixes
        ));
        // absolute item file (HIR keyspace)
        assert!(item_in_changed_set(
            "/var/mnt/ws/crates/cfdb-query/src/impact.rs",
            &changed,
            &suffixes
        ));
        // unrelated file
        assert!(!item_in_changed_set(
            "crates/cfdb-query/src/lib.rs",
            &changed,
            &suffixes
        ));
        // suffix must be on a path boundary — `bar.rs` must not match `xbar.rs`
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
        // empty changed set never matches
        assert!(!item_in_changed_set("crates/x/y.rs", &[], &[]));
    }
}

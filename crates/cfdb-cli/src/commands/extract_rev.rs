use std::path::{Path, PathBuf};
use std::process::Command;

use super::extract::extract_at_path;

pub(super) fn extract_at_rev(
    repo: &Path,
    rev: &str,
    db: &Path,
    keyspace: Option<String>,
    hir: bool,
    no_proc_macro: bool,
    profile: bool,
) -> Result<(), crate::CfdbCliError> {
    if !repo.join(".git").exists() && !repo.join(".git").is_file() {
        return Err(crate::CfdbCliError::Usage(format!(
            "--rev requires --workspace to point at a git repository root (no .git found under {})",
            repo.display()
        )));
    }
    let ks_name = keyspace.unwrap_or_else(|| short_rev(rev));
    let tmp = tempfile::tempdir()?;
    let worktree_path = tmp.path().join("worktree");
    let worktree_guard = GitWorktree::add(repo, &worktree_path, rev)?;

    eprintln!(
        "extract --rev {rev}: walking worktree {}",
        worktree_guard.path().display()
    );

    let result = extract_at_path(
        worktree_guard.path(),
        db,
        Some(ks_name),
        hir,
        no_proc_macro,
        profile,
    );

    worktree_guard.remove_soft_log(repo);
    result
}

pub(super) fn extract_at_url_rev(
    url_at_sha: &str,
    db: &Path,
    keyspace: Option<String>,
    hir: bool,
    no_proc_macro: bool,
    profile: bool,
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
    extract_at_path(&cache_dir, db, Some(ks_name), hir, no_proc_macro, profile)
}

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

fn clone_and_checkout(url: &str, sha: &str, cache_dir: &Path) -> Result<(), crate::CfdbCliError> {
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

pub(super) fn short_rev(rev: &str) -> String {
    if rev.len() > 12 && rev.chars().all(|c| c.is_ascii_hexdigit()) {
        rev[..12].to_string()
    } else {
        rev.replace(['/', ' ', '\t'], "_")
    }
}

pub(super) fn parse_url_at_sha(s: &str) -> Option<(&str, &str)> {
    let idx = s.rfind('@')?;
    let (url, at_sha) = s.split_at(idx);
    let sha = &at_sha[1..];
    if !url_has_scheme(url) {
        return None;
    }
    if sha.len() < 7 || !sha.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    Some((url, sha))
}

pub(super) fn is_url_at_sha(s: &str) -> bool {
    parse_url_at_sha(s).is_some()
}

fn url_has_scheme(url: &str) -> bool {
    url.starts_with("http://")
        || url.starts_with("https://")
        || url.starts_with("ssh://")
        || url.starts_with("file://")
}

pub(super) fn cache_dir_for(url: &str, sha: &str) -> PathBuf {
    cache_base_dir().join(url_hash_hex16(url)).join(sha)
}

pub(super) fn cache_base_dir() -> PathBuf {
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

pub(super) fn url_hash_hex16(url: &str) -> String {
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

struct GitWorktree {
    path: PathBuf,
    removed: bool,
}

impl GitWorktree {
    fn add(repo: &Path, path: &Path, rev: &str) -> Result<Self, crate::CfdbCliError> {
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
            let _ = Command::new("git")
                .args(["worktree", "remove", "--force"])
                .arg(&self.path)
                .status();
        }
    }
}

//! Extracted-file ground set for the `enrich-deprecation` dogfood.
//!
//! Subprocess-invokes `cfdb query` with `MATCH (f:File) RETURN f.path`
//! and parses the JSON output into the workspace-relative file list the
//! extractor actually walked. [`crate::grep_deprecated`] counts
//! `#[deprecated]` occurrences over exactly this set, so the sentinel's
//! source-side ground truth and the keyspace's graph-side count share
//! one file universe by construction — no re-implementation of cargo
//! target/module resolution in the harness.
//!
//! ## Pure-helper contract
//!
//! - **Input:** path to the `cfdb` binary, the keyspace `--db`
//!   directory, and the keyspace name.
//! - **Output:** sorted, deduplicated `Vec<String>` of
//!   workspace-relative `.rs` paths from the keyspace's `:File` nodes.
//! - **Error mode:** subprocess spawn / non-zero exit / unparseable
//!   JSON / zero `:File` nodes all bubble as `io::Error` → the harness
//!   maps to `EXIT_RUNTIME_ERROR` (1). A keyspace with zero files is a
//!   regression caught upstream by the extract + recall gates, never a
//!   reason to silently compare against an empty ground truth.
//!
//! ## Why subprocess, not direct keyspace read
//!
//! Same discipline as [`crate::count_items`] (RFC-039 §3.5.1) — the
//! harness never links `cfdb-cli`; it invokes `cfdb` exactly as the
//! violations sentinel will.

use std::io;
use std::path::Path;
use std::process::Command;

/// Subprocess-invoke `cfdb query` and return the sorted, deduplicated
/// set of workspace-relative file paths recorded on `:File` nodes.
pub fn file_paths_in_keyspace(
    cfdb_bin: &Path,
    db: &Path,
    keyspace: &str,
) -> io::Result<Vec<String>> {
    let output = Command::new(cfdb_bin)
        .arg("query")
        .arg("--db")
        .arg(db)
        .arg("--keyspace")
        .arg(keyspace)
        .arg("MATCH (f:File) RETURN f.path")
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(io::Error::other(format!(
            "cfdb query exited {}: {stderr}",
            output.status
        )));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_file_paths(&stdout).map_err(|reason| {
        io::Error::other(format!(
            "failed to parse cfdb query stdout: {reason}\nstdout: {stdout}"
        ))
    })
}

/// Pure JSON parser. Extracts every row's `f.path` string, sorts and
/// dedups. Split out for unit testing.
fn parse_file_paths(stdout: &str) -> Result<Vec<String>, String> {
    let value: serde_json::Value =
        serde_json::from_str(stdout).map_err(|e| format!("invalid JSON: {e}"))?;
    let rows = value
        .get("rows")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "no `rows` array in JSON".to_string())?;
    if rows.is_empty() {
        return Err("empty `rows` array — keyspace has zero :File nodes".to_string());
    }
    let mut paths = Vec::with_capacity(rows.len());
    for (idx, row) in rows.iter().enumerate() {
        let path = row
            .get("f.path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| format!("row {idx} missing string `f.path` column"))?;
        paths.push(path.to_string());
    }
    paths.sort();
    paths.dedup();
    Ok(paths)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_extracts_sorts_and_dedups_paths() {
        let stdout = r#"{"rows":[
            {"f.path":"crates/b/src/lib.rs"},
            {"f.path":"crates/a/src/lib.rs"},
            {"f.path":"crates/b/src/lib.rs"}
        ],"warnings":[]}"#;
        assert_eq!(
            parse_file_paths(stdout).unwrap(),
            vec![
                "crates/a/src/lib.rs".to_string(),
                "crates/b/src/lib.rs".to_string()
            ]
        );
    }

    #[test]
    fn parse_rejects_empty_rows() {
        let err = parse_file_paths(r#"{"rows":[],"warnings":[]}"#).unwrap_err();
        assert!(err.contains("zero :File nodes"), "unexpected error: {err}");
    }

    #[test]
    fn parse_rejects_missing_path_column() {
        let err = parse_file_paths(r#"{"rows":[{"path":"x.rs"}]}"#).unwrap_err();
        assert!(err.contains("`f.path` column"), "unexpected error: {err}");
    }

    #[test]
    fn parse_rejects_invalid_json() {
        let err = parse_file_paths("not-json").unwrap_err();
        assert!(err.contains("invalid JSON"), "unexpected error: {err}");
    }

    #[test]
    fn parse_rejects_missing_rows_array() {
        let err = parse_file_paths(r#"{"warnings":[]}"#).unwrap_err();
        assert!(err.contains("`rows` array"), "unexpected error: {err}");
    }
}

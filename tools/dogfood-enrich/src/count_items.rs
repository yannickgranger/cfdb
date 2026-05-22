//! Keyspace-side ground truth for the `enrich-bounded-context` dogfood.
//!
//! Subprocess-invokes `cfdb query` with a fixed
//! `MATCH (i:Item) WITH count(i) AS n RETURN n` expression and parses
//! the JSON output. The harness uses the count to derive the
//! `{{ nulls_threshold }}` substitution for
//! `.cfdb/queries/self-enrich-bounded-context.cypher` — Path B from
//! issue #355 (`{{ total_items }}` substitution path), the unblock
//! for #345 that avoids extending the cfdb-query subset with arithmetic.
//!
//! ## Pure-helper contract
//!
//! - **Input:** path to the `cfdb` binary, the keyspace `--db` directory,
//!   and the keyspace name.
//! - **Output:** `usize` count of `:Item` nodes in the keyspace.
//! - **Error mode:** subprocess spawn / non-zero exit / unparseable JSON
//!   / catastrophic empty-result (zero `:Item` nodes) all bubble as
//!   `io::Error`. The harness maps these to `EXIT_RUNTIME_ERROR` (1) —
//!   a workspace with zero items is itself a regression caught upstream
//!   by recall + extract gates.
//!
//! ## Why subprocess, not direct keyspace read
//!
//! `dogfood-enrich` is subprocess-driven by design (RFC-039 §3.5.1 — the
//! harness does not link `cfdb-cli` as a library). The same discipline
//! that keeps `cfdb_core::EnrichReport` as the only `cfdb-*` dep applies
//! here: the count helper invokes `cfdb` exactly as the violations
//! sentinel will, so a regression in `cfdb query` itself surfaces here
//! before the harness submits a sentinel that would also fail.

use std::io;
use std::path::Path;
use std::process::Command;

/// Subprocess-invoke `cfdb query` and return the count of `:Item` nodes
/// in the named keyspace. No kind filter — counts every `:Item`.
pub fn count_items_in_keyspace(cfdb_bin: &Path, db: &Path, keyspace: &str) -> io::Result<usize> {
    count_items_with_kind(cfdb_bin, db, keyspace, None)
}

/// Subprocess-invoke `cfdb query` and return the count of `:Item` nodes
/// matching the given `kind` filter (e.g. `Some("fn")` for the
/// reachability + metrics passes whose denominators are functions only).
/// `None` matches every kind.
///
/// `kind` is interpolated into the Cypher source — caller MUST pass a
/// hardcoded kind string (`"fn"`, `"struct"`, etc.), never user input.
/// All current call sites in `compute_extra_substitutions` use
/// compile-time string literals.
pub fn count_items_with_kind(
    cfdb_bin: &Path,
    db: &Path,
    keyspace: &str,
    kind: Option<&str>,
) -> io::Result<usize> {
    let cypher = match kind {
        Some(k) => format!("MATCH (i:Item) WHERE i.kind = \"{k}\" WITH count(i) AS n RETURN n"),
        None => "MATCH (i:Item) WITH count(i) AS n RETURN n".to_string(),
    };
    let output = Command::new(cfdb_bin)
        .arg("query")
        .arg("--db")
        .arg(db)
        .arg("--keyspace")
        .arg(keyspace)
        .arg(&cypher)
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(io::Error::other(format!(
            "cfdb query exited {}: {stderr}",
            output.status
        )));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_count(&stdout).map_err(|reason| {
        io::Error::other(format!(
            "failed to parse cfdb query stdout: {reason}\nstdout: {stdout}"
        ))
    })
}

/// Pure JSON parser. Extracts the integer value of the first row's `n`
/// column. Split out for unit testing.
fn parse_count(stdout: &str) -> Result<usize, String> {
    let value: serde_json::Value =
        serde_json::from_str(stdout).map_err(|e| format!("invalid JSON: {e}"))?;
    let rows = value
        .get("rows")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "no `rows` array in JSON".to_string())?;
    let first = rows
        .first()
        .ok_or_else(|| "empty `rows` array — keyspace has zero :Item nodes".to_string())?;
    let n = first
        .get("n")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| "first row missing integer `n` column".to_string())?;
    Ok(n as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_count_extracts_first_row_n() {
        let stdout = r#"{"rows":[{"n":1869}],"warnings":[]}"#;
        assert_eq!(parse_count(stdout).unwrap(), 1869);
    }

    #[test]
    fn parse_count_handles_pretty_printed_json() {
        let stdout = r#"{
  "rows": [
    {
      "n": 42
    }
  ]
}"#;
        assert_eq!(parse_count(stdout).unwrap(), 42);
    }

    #[test]
    fn parse_count_rejects_empty_rows() {
        let stdout = r#"{"rows":[],"warnings":[]}"#;
        let err = parse_count(stdout).unwrap_err();
        assert!(err.contains("zero :Item nodes"), "unexpected error: {err}");
    }

    #[test]
    fn parse_count_rejects_missing_n_column() {
        let stdout = r#"{"rows":[{"total":1869}]}"#;
        let err = parse_count(stdout).unwrap_err();
        assert!(err.contains("`n` column"), "unexpected error: {err}");
    }

    #[test]
    fn parse_count_rejects_invalid_json() {
        let stdout = "not-json";
        let err = parse_count(stdout).unwrap_err();
        assert!(err.contains("invalid JSON"), "unexpected error: {err}");
    }

    #[test]
    fn parse_count_rejects_missing_rows_array() {
        let stdout = r#"{"warnings":[]}"#;
        let err = parse_count(stdout).unwrap_err();
        assert!(err.contains("`rows` array"), "unexpected error: {err}");
    }
}

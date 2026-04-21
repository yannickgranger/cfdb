//! `cfdb check-predicate` verb handler — RFC-034 Slice 3 / #147.
//!
//! Loads `.cfdb/predicates/<name>.cypher` from `workspace_root`, resolves
//! `--param <name>:<form>:<value>` CLI args via the Slice-1
//! [`crate::param_resolver::resolve_params`] bridge, binds them into the
//! parsed [`cfdb_core::query::Query`], executes against the caller-pinned
//! keyspace, and emits the canonical three-column `(qname, line, reason)`
//! violation format — same exit contract as `cfdb violations`
//! (`row_count > 0` → non-zero exit at the dispatch layer).
//!
//! Determinism: rows are sorted by `(qname asc, line asc)` BEFORE
//! serialization so the output is stable regardless of future evaluator
//! tie-break changes (§4.1). `ORDER BY` inside the predicate template is
//! a necessary-but-not-sufficient condition.
//!
//! Visibility: `pub` per RFC-034 §3.1 + RATIFIED §9 (external verb
//! surface). Distinct from Slice-1's `pub(crate)` plumbing because this
//! is the integration-test / skill-consumer boundary.
//!
//! Output shapes:
//! - `text` (default): stderr summary `check-predicate: N (predicate: <name>)`;
//!   stdout `qname\tline\treason` per row.
//! - `json`: stdout pretty-printed [`PredicateRunReport`].

use std::path::{Path, PathBuf};

use cfdb_core::fact::PropValue;
use cfdb_core::result::RowValue;
use cfdb_core::store::StoreBackend;
use cfdb_query::parse;
use serde::Serialize;

use crate::compose;
use crate::param_resolver::resolve_params;
use crate::CfdbCliError;

/// Canonical column names emitted by `.cfdb/predicates/*.cypher` (RFC §3.5).
const QNAME_COL: &str = "qname";
const LINE_COL: &str = "line";
const REASON_COL: &str = "reason";

/// Report of one `check-predicate` invocation.
///
/// Serialization target when the user passes `--format json`; also the
/// library-API return value for programmatic consumers (tests, future
/// skill adapters). `rows` is deterministic ascending by `(qname, line)`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PredicateRunReport {
    /// Bare name as passed on the CLI (e.g. `"path-regex"`) — matches the
    /// basename of the `.cypher` file under `.cfdb/predicates/`.
    pub predicate_name: String,
    /// Absolute path of the `.cypher` file loaded for this invocation.
    /// Stable across runs so CI output can cite the exact source.
    pub predicate_path: PathBuf,
    /// Number of rows in `rows`. Exit-code contract at the dispatch layer
    /// checks this scalar — `> 0` → process exit 1.
    pub row_count: usize,
    /// One entry per violation row, sorted ascending by `(qname, line)`.
    pub rows: Vec<PredicateRow>,
}

/// Single row of a `check-predicate` result — mirrors the three-column
/// format `cfdb violations` emits so consumer skills can parse both with
/// the same code path.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct PredicateRow {
    /// Fully-qualified name (or path, for `:File` predicates) emitted by
    /// the predicate's `RETURN X AS qname` clause.
    pub qname: String,
    /// 1-based source line number, or `0` for predicates whose subject
    /// does not have a line (e.g. `:Crate`, `:File`).
    pub line: i64,
    /// Human-readable violation description from the predicate's
    /// `RETURN '...' AS reason` clause.
    pub reason: String,
}

/// Execute the named predicate against the pinned keyspace with
/// CLI-resolved params.
///
/// Errors surface as [`CfdbCliError`] variants:
/// - `Io` — predicate file missing / unreadable
/// - `Parse` — predicate file syntactically invalid
/// - `Store` — keyspace load / execute failure
/// - `Usage` — param resolution failure (unknown form, unknown context, …)
///   or malformed row shape (missing `qname` / `line` / `reason` column).
///
/// The caller (dispatch arm) inspects `report.row_count` and exits non-zero
/// when `> 0`. Printing is the caller's responsibility — this fn returns
/// the structured report; text or JSON rendering happens at the dispatch
/// boundary so the library-API path (integration tests, skills) can
/// inspect `rows` directly without parsing stdout.
pub fn check_predicate(
    db: &Path,
    keyspace: &str,
    workspace_root: &Path,
    name: &str,
    cli_params: &[String],
) -> Result<PredicateRunReport, CfdbCliError> {
    let predicate_path = predicate_path(workspace_root, name);
    let cypher = std::fs::read_to_string(&predicate_path).map_err(|e| {
        CfdbCliError::Usage(format!(
            "predicate `{name}` not found or unreadable at {}: {e}",
            predicate_path.display()
        ))
    })?;

    let mut parsed = parse(&cypher).map_err(|e| {
        CfdbCliError::Usage(format!(
            "parse error in predicate `{name}` ({}): {e}",
            predicate_path.display()
        ))
    })?;

    let resolved = resolve_params(workspace_root, cli_params)
        .map_err(|e| CfdbCliError::Usage(format!("{e}")))?;
    parsed.params.extend(resolved);

    let (store, ks) = compose::load_store(db, keyspace)?;
    let result = store.execute(&ks, &parsed)?;

    let mut rows: Vec<PredicateRow> = result
        .rows
        .iter()
        .map(|row| extract_predicate_row(row, name))
        .collect::<Result<Vec<_>, _>>()?;
    rows.sort();

    Ok(PredicateRunReport {
        predicate_name: name.to_string(),
        predicate_path,
        row_count: rows.len(),
        rows,
    })
}

/// Resolve `<workspace_root>/.cfdb/predicates/<name>.cypher`. Pure path
/// arithmetic; no I/O. Exposed as a helper so tests can reason about the
/// resolution without triggering a file read.
fn predicate_path(workspace_root: &Path, name: &str) -> PathBuf {
    workspace_root
        .join(".cfdb")
        .join("predicates")
        .join(format!("{name}.cypher"))
}

/// Map one evaluator row into a [`PredicateRow`]. Missing columns surface
/// as `CfdbCliError::Usage` rather than silent defaults so a broken
/// predicate (e.g. `RETURN c.name AS qanme` — typo) fails loudly.
fn extract_predicate_row(
    row: &cfdb_core::result::Row,
    predicate_name: &str,
) -> Result<PredicateRow, CfdbCliError> {
    let qname = extract_str(row, QNAME_COL).ok_or_else(|| {
        CfdbCliError::Usage(format!(
            "predicate `{predicate_name}` row is missing `{QNAME_COL}` string column; \
             RFC-034 §3.5 mandates `RETURN … AS qname, … AS line, … AS reason`"
        ))
    })?;
    let line = extract_i64(row, LINE_COL).ok_or_else(|| {
        CfdbCliError::Usage(format!(
            "predicate `{predicate_name}` row is missing `{LINE_COL}` integer column"
        ))
    })?;
    let reason = extract_str(row, REASON_COL).ok_or_else(|| {
        CfdbCliError::Usage(format!(
            "predicate `{predicate_name}` row is missing `{REASON_COL}` string column"
        ))
    })?;
    Ok(PredicateRow {
        qname,
        line,
        reason,
    })
}

fn extract_str(row: &cfdb_core::result::Row, col: &str) -> Option<String> {
    match row.get(col)? {
        RowValue::Scalar(PropValue::Str(s)) => Some(s.clone()),
        _ => None,
    }
}

fn extract_i64(row: &cfdb_core::result::Row, col: &str) -> Option<i64> {
    row.get(col).and_then(RowValue::as_i64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn predicate_row_sorts_by_qname_then_line_ascending() {
        let mut rows = [
            PredicateRow {
                qname: "beta".to_string(),
                line: 10,
                reason: "r".to_string(),
            },
            PredicateRow {
                qname: "alpha".to_string(),
                line: 20,
                reason: "r".to_string(),
            },
            PredicateRow {
                qname: "alpha".to_string(),
                line: 5,
                reason: "r".to_string(),
            },
        ];
        rows.sort();
        assert_eq!(
            rows.iter()
                .map(|r| (r.qname.as_str(), r.line))
                .collect::<Vec<_>>(),
            vec![("alpha", 5), ("alpha", 20), ("beta", 10)]
        );
    }

    #[test]
    fn predicate_run_report_serializes_to_stable_json() {
        let report = PredicateRunReport {
            predicate_name: "p".to_string(),
            predicate_path: PathBuf::from("/x/.cfdb/predicates/p.cypher"),
            row_count: 1,
            rows: vec![PredicateRow {
                qname: "q".to_string(),
                line: 1,
                reason: "r".to_string(),
            }],
        };
        let json = serde_json::to_value(&report).expect("serialize");
        assert_eq!(json["predicate_name"], "p");
        assert_eq!(json["row_count"], 1);
        assert_eq!(json["rows"][0]["qname"], "q");
        assert_eq!(json["rows"][0]["line"], 1);
        assert_eq!(json["rows"][0]["reason"], "r");
    }

    #[test]
    fn missing_predicate_file_returns_structured_usage_error() {
        let tmp = tempdir().unwrap();
        let db = tmp.path().join("db");
        fs::create_dir_all(&db).unwrap();
        let err = check_predicate(&db, "cfdb", tmp.path(), "nonexistent", &[]).unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("predicate `nonexistent` not found"),
            "expected not-found message, got: {msg}"
        );
    }

    #[test]
    fn predicate_path_resolution_is_deterministic() {
        let p1 = predicate_path(Path::new("/ws"), "my-predicate");
        let p2 = predicate_path(Path::new("/ws"), "my-predicate");
        assert_eq!(p1, p2);
        assert_eq!(
            p1,
            PathBuf::from("/ws/.cfdb/predicates/my-predicate.cypher")
        );
    }
}

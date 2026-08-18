//! `cfdb check --trigger <ID>` — the editorial-drift trigger verb.
//!
//! The handler loads the keyspace once, runs
//! `cfdb_classify::ClassifyEngine::check`, prints the `violations:` summary
//! line and serialises the report as the merged `QueryResult` payload. The
//! triggers themselves live in `cfdb-classify`.

use std::path::Path;

use cfdb_classify::TriggerId;
use cfdb_core::result::QueryResult;

use crate::compose;
use crate::output;

/// `cfdb check --trigger <ID> --db <path> --keyspace <name>` entry.
/// Returns the total row count so the clap dispatch arm can apply the
/// same exit-30-on-rows rule that `Command::Violations` uses.
///
/// Prints to stderr the `violations: N (rule: trigger <ID>)` summary
/// line; prints to stdout the pretty JSON of the rows + warnings.
pub fn check(db: &Path, keyspace: &str, trigger: TriggerId) -> Result<usize, crate::CfdbCliError> {
    let (store, ks) = compose::load_store(db, keyspace)?;
    let report = compose::classify_engine(&store).check(&ks, trigger)?;

    let row_count = report.row_count();
    eprintln!("violations: {row_count} (rule: trigger {trigger})");

    let payload = QueryResult {
        rows: report.rows,
        warnings: report.warnings,
    };
    output::emit_json(&payload)?;

    Ok(row_count)
}

use std::path::Path;

use cfdb_classify::TriggerId;
use cfdb_core::result::QueryResult;

use crate::compose;
use crate::output;

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

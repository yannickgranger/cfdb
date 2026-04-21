//! Cypher-rule command handlers — `cfdb violations` + the shared
//! `run_cypher_rule` / `parse_and_execute` plumbing consumed by
//! `crate::check` and `crate::check_predicate`. Split out of
//! `commands.rs` for the drift god-file decomposition (#151). Move-only;
//! visibility preserved (`pub(crate)` is load-bearing).

use std::path::{Path, PathBuf};

use cfdb_core::store::StoreBackend;
use cfdb_query::{lint_shape, parse, ShapeLint};

use crate::compose;

/// Run a .cypher rule file and print violations. Returns the number of
/// rows found so the caller can set the process exit code.
///
/// Prints to stderr (always):
/// - A shape-lint warning if one fires on the rule (same as `cfdb query`).
/// - A human-readable `violations: N (rule: <path>)` summary line.
///
/// Prints to stdout:
/// - Default: pretty-printed JSON of the full `QueryResult` (rows +
///   warnings) so callers can parse it programmatically.
/// - When `count_only` is set: the integer row count on its own line,
///   suitable for capture by `rows=$(cfdb violations ... --count-only)`
///   in CI scripts like `ci/cross-dogfood.sh` (RFC-033 §3.2). The
///   JSON payload is suppressed in this mode — the caller already
///   knows the rule file path and wants only the terse count.
pub fn violations(
    db: PathBuf,
    keyspace: String,
    rule: PathBuf,
    count_only: bool,
) -> Result<usize, crate::CfdbCliError> {
    let cypher = std::fs::read_to_string(&rule)
        .map_err(|e| format!("read rule file {}: {e}", rule.display()))?;
    let rule_tag = rule.display().to_string();
    run_cypher_rule(&db, &keyspace, &cypher, &rule_tag, count_only)
}

/// Shared cypher-rule plumbing — parse, shape-lint, execute, and print
/// rows. Used by both the file-based `violations` verb and the embedded-
/// rule `check --trigger` verb so the `parse → execute → print → rows` pipeline
/// lives in exactly one place (EXTEND decision per `.prescriptions/101.md`).
///
/// `rule_tag` appears in the stderr summary line: for file-based rules
/// callers pass the file path; for embedded-rule triggers callers pass
/// e.g. `"trigger T1"` so the summary reads `violations: N (rule: trigger T1)`.
fn run_cypher_rule(
    db: &Path,
    keyspace: &str,
    cypher: &str,
    rule_tag: &str,
    count_only: bool,
) -> Result<usize, crate::CfdbCliError> {
    let result = parse_and_execute(db, keyspace, cypher, rule_tag)?;
    let row_count = result.rows.len();
    eprintln!("violations: {row_count} (rule: {rule_tag})");

    if count_only {
        println!("{row_count}");
    } else {
        let as_json = serde_json::to_string_pretty(&result)?;
        println!("{as_json}");
    }

    Ok(row_count)
}

/// Parse a cypher string, run shape-lint (logging any warnings to
/// stderr), load the keyspace, and execute. Returns the raw
/// [`cfdb_core::result::QueryResult`] so callers that need to merge
/// multiple rule results before printing — e.g. the `cfdb check
/// --trigger T1` verb — can do so without going through
/// [`run_cypher_rule`]'s print path.
///
/// `rule_tag` appears in the parse-error message so a malformed
/// embedded trigger rule identifies itself the same way a file-path
/// rule does.
pub(crate) fn parse_and_execute(
    db: &Path,
    keyspace: &str,
    cypher: &str,
    rule_tag: &str,
) -> Result<cfdb_core::result::QueryResult, crate::CfdbCliError> {
    let parsed = parse(cypher).map_err(|e| format!("parse error in {rule_tag}: {e}"))?;
    let lints = lint_shape(&parsed);
    for lint in &lints {
        match lint {
            ShapeLint::CartesianFunctionEquality {
                message,
                suggestion,
            } => {
                eprintln!("shape-lint: {message}");
                eprintln!("  suggestion: {suggestion}");
            }
            _ => eprintln!("shape-lint: {lint:?}"),
        }
    }

    let (store, ks) = compose::load_store(db, keyspace)?;
    let result = store.execute(&ks, &parsed)?;
    Ok(result)
}

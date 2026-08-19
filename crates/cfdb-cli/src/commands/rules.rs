use std::path::{Path, PathBuf};

use cfdb_core::store::QueryBackend;
use cfdb_query::{lint_shape, parse, ShapeLint};

use crate::compose;
use crate::output;

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
        output::emit_json(&result)?;
    }

    Ok(row_count)
}

fn parse_and_execute(
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
    let result = compose::query_engine(&store).execute(&ks, &parsed)?;
    Ok(result)
}

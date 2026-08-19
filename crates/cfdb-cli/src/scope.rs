use std::path::Path;
use std::str::FromStr;

use cfdb_classify::{ExplainSink, ScopeInventory, ScopeOptions};

use crate::compose;
use crate::output;
use crate::output::OutputFormat;

#[allow(clippy::too_many_arguments)]
pub fn scope(
    db: &Path,
    context: &str,
    workspace: Option<&Path>,
    format: &str,
    output: Option<&Path>,
    keyspace: Option<&str>,
    explain: bool,
    production_only: bool,
) -> Result<(), crate::CfdbCliError> {
    if OutputFormat::from_str(format).ok() != Some(OutputFormat::Json) {
        return Err(format!(
            "`--format {format}` is not supported in v0.1. \
             Only `json` ships today; `table` is deferred to v0.2 per §A3.3."
        )
        .into());
    }

    let ks_name = resolve_keyspace_name(db, keyspace)?;
    compose::ensure_keyspace_exists(db, &ks_name)?;

    let (store, ks) = match workspace {
        Some(ws) => compose::load_store_with_workspace(db, &ks_name, Some(ws.to_path_buf()))?,
        None => compose::load_store(db, &ks_name)?,
    };
    let engine = compose::classify_engine(&store);
    let sink = if explain {
        ExplainSink::enabled()
    } else {
        ExplainSink::disabled()
    };
    let opts = ScopeOptions { production_only };
    let inventory = engine.scope(&ks, context, &opts, Some(&sink))?;
    if explain {
        for row in sink.drain() {
            eprintln!("{}", row.format_line());
        }
    }
    emit_scope_output(&inventory, output)
}

fn emit_scope_output(
    inventory: &ScopeInventory,
    output_path: Option<&Path>,
) -> Result<(), crate::CfdbCliError> {
    match output_path {
        Some(path) => {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("create output parent dir `{}`: {e}", parent.display()))?;
            }
            let json = serde_json::to_string_pretty(inventory)?;
            std::fs::write(path, &json)
                .map_err(|e| format!("write output file `{}`: {e}", path.display()))?;
        }
        None => {
            output::emit_json(inventory)?;
        }
    }
    Ok(())
}

pub(crate) fn resolve_keyspace_name(
    db: &Path,
    keyspace: Option<&str>,
) -> Result<String, crate::CfdbCliError> {
    if let Some(name) = keyspace {
        return Ok(name.to_string());
    }
    if !db.exists() {
        return Err(format!("db directory `{}` does not exist", db.display()).into());
    }
    let names = compose::list_keyspace_names(db)?;
    match names.len() {
        0 => Err(format!(
            "db `{}` contains no keyspace files; run `cfdb extract` first",
            db.display()
        )
        .into()),
        1 => Ok(names.into_iter().next().expect("len==1 — just checked")),
        n => Err(format!(
            "db `{}` contains {n} keyspaces; pass --keyspace to disambiguate",
            db.display()
        )
        .into()),
    }
}

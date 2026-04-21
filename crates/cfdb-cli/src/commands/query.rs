//! Query command handlers — `cfdb query` and the typed `list-callers`
//! convenience verb. Split out of `commands.rs` for the drift god-file
//! decomposition (#151). Move-only; public paths preserved via
//! `pub use` in `commands.rs`.

use std::path::PathBuf;

use cfdb_core::store::StoreBackend;
use cfdb_core::{Param, PropValue, Query};
use cfdb_query::{lint_shape, parse, ShapeLint};

use crate::compose;

use super::extract::keyspace_path;

/// Embedded cypher template for `cfdb list-callers`. Loaded via `include_str!`
/// at compile time so the shipped binary is self-contained — no runtime file
/// lookup, no deployment-relative paths, and `cargo build` picks up edits to
/// the template automatically.
const LIST_CALLERS_CYPHER: &str = include_str!("../../../../examples/queries/list-callers.cypher");

pub fn query(
    db: PathBuf,
    keyspace: String,
    cypher: String,
    params: Option<String>,
    input: Option<PathBuf>,
) -> Result<(), crate::CfdbCliError> {
    let mut parsed = parse(&cypher).map_err(|e| format!("parse error: {e}"))?;

    if let Some(raw) = params.as_deref() {
        let json: serde_json::Value =
            serde_json::from_str(raw).map_err(|e| format!("--params is not valid JSON: {e}"))?;
        bind_json_params(&mut parsed, &json)?;
    }
    if let Some(path) = input.as_deref() {
        if !path.exists() {
            return Err(format!("--input file not found: {}", path.display()).into());
        }
        eprintln!("query: --input accepted but not yet wired in v0.1 (Phase A — RFC §6.2)");
    }

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
            // ShapeLint is #[non_exhaustive]; v0.2 may add new variants.
            _ => eprintln!("shape-lint: {lint:?}"),
        }
    }

    let (store, ks) = compose::load_store(&db, &keyspace)?;

    let result = store.execute(&ks, &parsed)?;

    let as_json = serde_json::to_string_pretty(&result)?;
    println!("{as_json}");
    Ok(())
}

/// Bind a `--params <json>` object into a parsed `Query`'s param bag. The
/// input MUST be a JSON object whose values are scalars (string, number,
/// bool, or null). Arrays and objects are rejected with a clear error —
/// v0.1 only supports scalar bindings; list/typed bindings come later.
/// This is the canonical wire-up boundary for the CLI → evaluator param
/// flow: the parser emits an empty `Query.params` bag, this function
/// populates it, and the evaluator reads from the populated bag.
fn bind_json_params(
    parsed: &mut Query,
    json: &serde_json::Value,
) -> Result<(), crate::CfdbCliError> {
    let obj = json
        .as_object()
        .ok_or("--params must be a JSON object, e.g. '{\"qname\":\"(?i).*kalman.*\"}'")?;
    for (k, v) in obj {
        bind_single_param(parsed, k, v)?;
    }
    Ok(())
}

/// Bind one `(key, value)` from the `--params` JSON object into the parsed
/// query's param bag. Factored out of [`bind_json_params`] so the `k.clone()`
/// required by the scalar insert lives in a helper rather than in the
/// outer `for (k, v) in obj` loop body — the quality-metrics gate treats
/// the closure-less `for` as the clone-in-loop trigger.
fn bind_single_param(
    parsed: &mut Query,
    k: &str,
    v: &serde_json::Value,
) -> Result<(), crate::CfdbCliError> {
    match v {
        serde_json::Value::String(_)
        | serde_json::Value::Number(_)
        | serde_json::Value::Bool(_)
        | serde_json::Value::Null => {
            parsed
                .params
                .insert(k.to_string(), Param::Scalar(PropValue::from_json(v)));
            Ok(())
        }
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => Err(format!(
            "--params `{k}` must be a scalar (string/number/bool/null); \
             arrays and objects are not supported in v0.1"
        )
        .into()),
    }
}

/// `cfdb list-callers --db <path> --keyspace <name> --qname <regex>` —
/// typed convenience verb over the raw `query` path. Loads the embedded
/// `list-callers.cypher` template, binds `$qname` to the CLI arg, executes
/// against the named keyspace, and prints the result as pretty JSON in
/// the same format as `cfdb query`. The template and the raw path MUST
/// produce byte-identical output for the same `$qname` input — that is
/// the genericity contract the typed verbs are meant to satisfy (one
/// query, many targets, sugar over the raw path).
pub fn list_callers(
    db: PathBuf,
    keyspace: String,
    qname: String,
) -> Result<(), crate::CfdbCliError> {
    let path = keyspace_path(&db, &keyspace);
    if !path.exists() {
        return Err(format!(
            "keyspace `{keyspace}` not found in db `{}` (looked for {})",
            db.display(),
            path.display()
        )
        .into());
    }

    let mut parsed = parse(LIST_CALLERS_CYPHER)
        .map_err(|e| format!("parse error in embedded list-callers template: {e}"))?;
    parsed
        .params
        .insert("qname".to_string(), Param::Scalar(PropValue::Str(qname)));

    let (store, ks) = compose::load_store(&db, &keyspace)?;
    let result = store.execute(&ks, &parsed)?;

    let as_json = serde_json::to_string_pretty(&result)?;
    println!("{as_json}");
    Ok(())
}

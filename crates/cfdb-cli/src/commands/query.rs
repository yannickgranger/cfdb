use std::path::PathBuf;

use cfdb_core::store::QueryBackend;
use cfdb_core::{ParamBinding, PropValue, Query};
use cfdb_query::{lint_shape, parse, ShapeLint};

use crate::compose;
use crate::output;

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
            _ => eprintln!("shape-lint: {lint:?}"),
        }
    }

    let (store, ks) = compose::load_store(&db, &keyspace)?;

    let result = compose::query_engine(&store).execute(&ks, &parsed)?;

    output::emit_json(&result)
}

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
                .insert(k.to_string(), ParamBinding::Scalar(PropValue::from_json(v)));
            Ok(())
        }
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => Err(format!(
            "--params `{k}` must be a scalar (string/number/bool/null); \
             arrays and objects are not supported in v0.1"
        )
        .into()),
    }
}

pub fn list_callers(
    db: PathBuf,
    keyspace: String,
    qname: String,
) -> Result<(), crate::CfdbCliError> {
    compose::ensure_keyspace_exists(&db, &keyspace)?;

    let mut parsed = parse(LIST_CALLERS_CYPHER)
        .map_err(|e| format!("parse error in embedded list-callers template: {e}"))?;
    parsed.params.insert(
        "qname".to_string(),
        ParamBinding::Scalar(PropValue::Str(qname)),
    );

    let (store, ks) = compose::load_store(&db, &keyspace)?;
    let result = compose::query_engine(&store).execute(&ks, &parsed)?;

    output::emit_json(&result)
}

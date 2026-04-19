//! Core ingest / query / dump command handlers.
//!
//! Split out of `lib.rs` for the god-file decomposition (#3751). Public
//! surface preserved: every item here is re-exported from the crate root.

use std::path::{Path, PathBuf};

use cfdb_core::schema::Keyspace;
use cfdb_core::store::StoreBackend;
use cfdb_core::{Param, PropValue, Query};
use cfdb_petgraph::{persist, PetgraphStore};
use cfdb_query::{lint_shape, parse, ShapeLint};

/// Embedded cypher template for `cfdb list-callers`. Loaded via `include_str!`
/// at compile time so the shipped binary is self-contained — no runtime file
/// lookup, no deployment-relative paths, and `cargo build` picks up edits to
/// the template automatically.
const LIST_CALLERS_CYPHER: &str = include_str!("../../../examples/queries/list-callers.cypher");

pub fn keyspace_path(db: &Path, keyspace: &str) -> PathBuf {
    db.join(format!("{keyspace}.json"))
}

pub fn extract(
    workspace: PathBuf,
    db: PathBuf,
    keyspace: Option<String>,
) -> Result<(), crate::CfdbCliError> {
    let ks_name = keyspace.unwrap_or_else(|| {
        workspace
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("default")
            .to_string()
    });
    let ks = Keyspace::new(&ks_name);

    eprintln!("extract: walking {}", workspace.display());
    let (nodes, edges) = cfdb_extractor::extract_workspace(&workspace)?;
    eprintln!("extract: {} nodes, {} edges", nodes.len(), edges.len());

    let mut store = PetgraphStore::new();
    store.ingest_nodes(&ks, nodes)?;
    store.ingest_edges(&ks, edges)?;

    std::fs::create_dir_all(&db)?;
    let path = keyspace_path(&db, &ks_name);
    persist::save(&store, &ks, &path)?;
    eprintln!("extract: saved keyspace `{ks_name}` to {}", path.display());
    Ok(())
}

pub fn query(
    db: PathBuf,
    keyspace: String,
    cypher: String,
    params: Option<String>,
    input: Option<PathBuf>,
) -> Result<(), crate::CfdbCliError> {
    let ks = Keyspace::new(&keyspace);
    let path = keyspace_path(&db, &keyspace);

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

    let mut store = PetgraphStore::new();
    persist::load(&mut store, &ks, &path)?;

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
        match v {
            serde_json::Value::String(_)
            | serde_json::Value::Number(_)
            | serde_json::Value::Bool(_)
            | serde_json::Value::Null => {
                parsed
                    .params
                    .insert(k.clone(), Param::Scalar(PropValue::from_json(v)));
            }
            serde_json::Value::Array(_) | serde_json::Value::Object(_) => {
                return Err(format!(
                    "--params `{k}` must be a scalar (string/number/bool/null); \
                     arrays and objects are not supported in v0.1"
                )
                .into());
            }
        }
    }
    Ok(())
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
    let ks = Keyspace::new(&keyspace);
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

    let mut store = PetgraphStore::new();
    persist::load(&mut store, &ks, &path)?;
    let result = store.execute(&ks, &parsed)?;

    let as_json = serde_json::to_string_pretty(&result)?;
    println!("{as_json}");
    Ok(())
}

/// Run a .cypher rule file and print violations as pretty JSON. Returns
/// the number of rows found so the caller can set the process exit code.
///
/// Prints to stderr:
/// - A shape-lint warning if one fires on the rule (same as `cfdb query`).
/// - A human-readable `violations: N (rule: <path>)` summary line.
///
/// Prints to stdout:
/// - Pretty-printed JSON of the full `QueryResult` (rows + warnings) so
///   callers can parse it programmatically.
pub fn violations(
    db: PathBuf,
    keyspace: String,
    rule: PathBuf,
) -> Result<usize, crate::CfdbCliError> {
    let cypher = std::fs::read_to_string(&rule)
        .map_err(|e| format!("read rule file {}: {e}", rule.display()))?;

    let ks = Keyspace::new(&keyspace);
    let path = keyspace_path(&db, &keyspace);

    let parsed = parse(&cypher).map_err(|e| format!("parse error in {}: {e}", rule.display()))?;
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

    let mut store = PetgraphStore::new();
    persist::load(&mut store, &ks, &path)?;
    let result = store.execute(&ks, &parsed)?;

    let row_count = result.rows.len();
    eprintln!("violations: {row_count} (rule: {})", rule.display());

    let as_json = serde_json::to_string_pretty(&result)?;
    println!("{as_json}");

    Ok(row_count)
}

pub fn dump(db: PathBuf, keyspace: String) -> Result<(), crate::CfdbCliError> {
    let ks = Keyspace::new(&keyspace);
    let path = keyspace_path(&db, &keyspace);

    let mut store = PetgraphStore::new();
    persist::load(&mut store, &ks, &path)?;

    let dump = store.canonical_dump(&ks)?;
    println!("{dump}");
    Ok(())
}

pub fn list_keyspaces(db: PathBuf) -> Result<(), crate::CfdbCliError> {
    if !db.exists() {
        return Ok(());
    }
    let mut names: Vec<String> = std::fs::read_dir(&db)?
        .filter_map(|entry| entry.ok())
        .filter_map(|e| {
            let path = e.path();
            if path.extension().and_then(|s| s.to_str()) == Some("json") {
                path.file_stem().and_then(|s| s.to_str()).map(String::from)
            } else {
                None
            }
        })
        .collect();
    names.sort();
    for n in names {
        println!("{n}");
    }
    Ok(())
}

/// `cfdb export` — alias of `cfdb dump` with a `--format` flag for forward
/// compatibility. v0.1 only supports `sorted-jsonl` (the canonical dump).
pub fn export(
    db: PathBuf,
    keyspace: String,
    format: &str,
) -> Result<(), crate::CfdbCliError> {
    if format != "sorted-jsonl" {
        return Err(format!("unsupported --format `{format}`. v0.1 supports: sorted-jsonl").into());
    }
    dump(db, keyspace)
}

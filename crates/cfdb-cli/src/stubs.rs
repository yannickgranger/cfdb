use std::path::{Path, PathBuf};

use cfdb_core::query::ItemKind;
use cfdb_core::result::{Warning, WarningKind};
use cfdb_core::schema::schema_describe;
use cfdb_core::store::{QueryBackend, StoreBackend};
use cfdb_query::list_items_matching as compose_list_items_matching;

use crate::compose;
use crate::output;

pub fn typed_stub(
    verb: &str,
    db: &Path,
    keyspace: &str,
    args: &[(&str, &str)],
) -> Result<(), crate::CfdbCliError> {
    compose::ensure_keyspace_exists(db, keyspace)?;
    let mut report = serde_json::Map::new();
    report.insert("verb".into(), serde_json::Value::String(verb.into()));
    report.insert("ran".into(), serde_json::Value::Bool(false));
    report.insert(
        "warnings".into(),
        serde_json::Value::Array(vec![serde_json::Value::String(format!(
            "{verb}: typed convenience verb not implemented in v0.1 (Phase A — wired \
             in v0.2 / Phase B per EPIC #3622)"
        ))]),
    );
    report.insert(
        "keyspace".into(),
        serde_json::Value::String(keyspace.to_string()),
    );
    for (k, v) in args {
        report.insert(
            (*k).to_string(),
            serde_json::Value::String((*v).to_string()),
        );
    }
    output::emit_json(&report)
}

pub fn list_items_matching(
    db: &Path,
    keyspace: &str,
    name_pattern: &str,
    kinds: Option<&[ItemKind]>,
    group_by_context: bool,
) -> Result<(), crate::CfdbCliError> {
    compose::ensure_keyspace_exists(db, keyspace)?;

    let (store, ks) = compose::load_store(db, keyspace)?;

    let query = compose_list_items_matching(name_pattern, kinds, group_by_context);
    let mut result = compose::query_engine(&store).execute(&ks, &query)?;

    if let Some(ks) = kinds {
        if ks.iter().any(|k| matches!(k, ItemKind::ImplBlock)) {
            result.warn(Warning {
                kind: WarningKind::EmptyResult,
                message: "kind `ImplBlock` is not emitted by the cfdb syn extractor in v0.1 \
                          (only nested methods are emitted); filter matched 0 items for that kind"
                    .to_string(),
                suggestion: Some(
                    "Remove `ImplBlock` from --kinds, or wait for v0.2 HIR-aware emission."
                        .to_string(),
                ),
            });
        }
    }

    output::emit_json(&result)
}

pub fn snapshots(db: PathBuf) -> Result<(), crate::CfdbCliError> {
    if !db.exists() {
        println!("[]");
        return Ok(());
    }
    let mut entries: Vec<serde_json::Value> = Vec::new();
    let names = compose::list_keyspace_names(&db)?;
    for name in names {
        let mut row = serde_json::Map::new();
        row.insert("keyspace".into(), serde_json::Value::String(name));
        row.insert(
            "schema_version".into(),
            serde_json::Value::String(cfdb_core::SchemaVersion::CURRENT.to_string()),
        );
        row.insert("sha".into(), serde_json::Value::Null);
        row.insert("timestamp".into(), serde_json::Value::Null);
        entries.push(serde_json::Value::Object(row));
    }
    output::emit_json(&entries)
}

pub fn drop_keyspace_cmd(db: PathBuf, keyspace: String) -> Result<(), crate::CfdbCliError> {
    let path = compose::ensure_keyspace_exists(&db, &keyspace)?;
    let (mut store, ks) = compose::load_store(&db, &keyspace)?;
    store.drop_keyspace(&ks)?;
    std::fs::remove_file(&path)?;
    eprintln!("drop: removed keyspace `{keyspace}` ({})", path.display());
    Ok(())
}

pub fn schema_describe_cmd() -> Result<(), crate::CfdbCliError> {
    let describe = schema_describe();
    output::emit_json(&describe)
}

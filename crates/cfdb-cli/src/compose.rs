use std::path::{Path, PathBuf};

#[cfg(feature = "classify")]
use cfdb_classify::ClassifyEngine;
use cfdb_core::schema::Keyspace;
use cfdb_eval::QueryEngine;
use cfdb_petgraph::index::spec::IndexSpec;
use cfdb_petgraph::{persist, PetgraphStore};

use crate::commands::keyspace_path;
use crate::CfdbCliError;

const INDEXES_TOML_PATH: &str = ".cfdb/indexes.toml";

pub(crate) fn empty_store() -> PetgraphStore {
    PetgraphStore::new()
}

pub(crate) fn query_engine(store: &PetgraphStore) -> QueryEngine<'_, PetgraphStore> {
    QueryEngine::new(store)
}

#[cfg(feature = "classify")]
pub(crate) fn classify_engine(store: &PetgraphStore) -> ClassifyEngine<'_, PetgraphStore> {
    ClassifyEngine::new(store)
}

pub(crate) fn ensure_keyspace_exists(db: &Path, keyspace: &str) -> Result<PathBuf, CfdbCliError> {
    let path = keyspace_path(db, keyspace);
    if !path.exists() {
        return Err(format!(
            "keyspace `{keyspace}` not found in db `{}` (looked for {})",
            db.display(),
            path.display()
        )
        .into());
    }
    Ok(path)
}

pub(crate) fn load_store(
    db: &Path,
    keyspace: &str,
) -> Result<(PetgraphStore, Keyspace), CfdbCliError> {
    let auto_workspace = discover_workspace_from_db(db);
    load_store_with_workspace(db, keyspace, auto_workspace)
}

fn discover_workspace_from_db(db: &Path) -> Option<PathBuf> {
    let canonical = db.canonicalize().ok()?;
    for ancestor in canonical.ancestors() {
        if ancestor.join(INDEXES_TOML_PATH).is_file() {
            return Some(ancestor.to_path_buf());
        }
    }
    None
}

pub(crate) fn load_store_with_workspace(
    db: &Path,
    keyspace: &str,
    workspace_root: Option<PathBuf>,
) -> Result<(PetgraphStore, Keyspace), CfdbCliError> {
    let ks = Keyspace::new(keyspace);
    let path = keyspace_path(db, keyspace);
    let mut store = match workspace_root {
        Some(root) => {
            let spec = IndexSpec::from_path(&root.join(INDEXES_TOML_PATH))
                .map_err(|e| CfdbCliError::from(format!("load .cfdb/indexes.toml: {e}")))?;
            PetgraphStore::new().with_workspace(root).with_indexes(spec)
        }
        None => empty_store(),
    };
    persist::load(&mut store, &ks, &path)?;
    Ok((store, ks))
}

pub(crate) fn save_store(
    store: &PetgraphStore,
    keyspace: &Keyspace,
    db: &Path,
) -> Result<PathBuf, CfdbCliError> {
    std::fs::create_dir_all(db)?;
    let path = keyspace_path(db, keyspace.as_str());
    persist::save(store, keyspace, &path)?;
    Ok(path)
}

pub(crate) fn list_keyspace_names(db: &Path) -> Result<Vec<String>, CfdbCliError> {
    if !db.exists() {
        return Ok(Vec::new());
    }
    let mut names: Vec<String> = std::fs::read_dir(db)?
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let p = e.path();
            if p.extension().and_then(|s| s.to_str()) == Some("json") {
                p.file_stem().and_then(|s| s.to_str()).map(String::from)
            } else {
                None
            }
        })
        .collect();
    names.sort();
    Ok(names)
}

#[cfg(test)]
mod tests {

    use cfdb_core::schema::Keyspace;
    use cfdb_core::store::StoreBackend;

    use super::*;

    const SAMPLE_INDEXES_TOML: &str = r#"
[[index]]
label = "Item"
prop = "qname"
notes = "slice-7 compose test"

[[index]]
label = "Item"
computed = "last_segment(qname)"
notes = "slice-7 compose test"
"#;

    fn seed_db(db: &Path, keyspace: &str) {
        std::fs::create_dir_all(db).expect("mkdir db");
        let ks = Keyspace::new(keyspace);
        let mut store = PetgraphStore::new();
        StoreBackend::ingest_nodes(&mut store, &ks, Vec::new()).expect("seed ingest");
        save_store(&store, &ks, db).expect("seed save_store");
    }

    #[test]
    fn load_store_with_workspace_none_has_empty_spec() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let db = tmp.path().join("db");
        seed_db(&db, "ks0");
        let (store, _ks) = load_store_with_workspace(&db, "ks0", None).expect("load");
        assert!(
            store.workspace_root().is_none(),
            "workspace=None must leave workspace_root unset"
        );
        assert!(
            store.index_spec().is_empty(),
            "workspace=None must yield IndexSpec::empty()"
        );
    }

    #[test]
    fn load_store_with_workspace_some_missing_toml_is_empty_spec() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let db = tmp.path().join("db");
        let ws = tmp.path().join("ws");
        std::fs::create_dir_all(&ws).expect("mkdir ws");
        seed_db(&db, "ks1");
        let (store, _ks) =
            load_store_with_workspace(&db, "ks1", Some(ws)).expect("load with missing toml");
        assert!(
            store.workspace_root().is_some(),
            "workspace_root must be set even when indexes.toml is missing"
        );
        assert!(
            store.index_spec().is_empty(),
            "missing .cfdb/indexes.toml must yield IndexSpec::empty(), not error"
        );
    }

    #[test]
    fn load_store_with_workspace_some_reads_indexes_toml() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let db = tmp.path().join("db");
        let ws = tmp.path().join("ws");
        std::fs::create_dir_all(ws.join(".cfdb")).expect("mkdir .cfdb");
        std::fs::write(ws.join(".cfdb/indexes.toml"), SAMPLE_INDEXES_TOML).expect("write toml");
        seed_db(&db, "ks2");

        let (store, _ks) = load_store_with_workspace(&db, "ks2", Some(ws)).expect("load with toml");
        assert_eq!(
            store.index_spec().entries.len(),
            2,
            "IndexSpec must contain the two [[index]] entries from the sample TOML"
        );
    }

    #[test]
    fn load_store_with_workspace_invalid_toml_errors() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let db = tmp.path().join("db");
        let ws = tmp.path().join("ws");
        std::fs::create_dir_all(ws.join(".cfdb")).expect("mkdir .cfdb");
        std::fs::write(ws.join(".cfdb/indexes.toml"), "this is not valid toml = [").expect("write");
        seed_db(&db, "ks3");
        let result = load_store_with_workspace(&db, "ks3", Some(ws));
        assert!(
            result.is_err(),
            "malformed .cfdb/indexes.toml must propagate as CfdbCliError"
        );
    }
}

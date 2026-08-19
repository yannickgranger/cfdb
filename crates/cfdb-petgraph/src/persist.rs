use std::fs;
use std::path::Path;

use cfdb_core::fact::{Edge, Node};
use cfdb_core::result::{Warning, WarningKind};
use cfdb_core::schema::{Keyspace, SchemaVersion};
use cfdb_core::store::StoreBackend;
use cfdb_core::store::StoreError;
use serde::{Deserialize, Serialize};

use crate::PetgraphStore;

#[derive(Debug, Serialize, Deserialize)]
pub struct KeyspaceFile {
    pub schema_version: SchemaVersion,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub contention_warnings: Vec<Warning>,
}

pub fn save(store: &PetgraphStore, keyspace: &Keyspace, path: &Path) -> Result<(), StoreError> {
    let (mut nodes, mut edges) = store.export(keyspace)?;
    nodes.sort_by(|a, b| a.sort_key().cmp(&b.sort_key()));
    edges.sort_by(|a, b| a.sort_key().cmp(&b.sort_key()));

    let schema_version = store
        .schema_version(keyspace)
        .unwrap_or(SchemaVersion::V0_1_0);
    let file = KeyspaceFile {
        schema_version,
        nodes,
        edges,
        contention_warnings: store
            .ingest_warnings(keyspace)
            .into_iter()
            .filter(|w| w.kind == WarningKind::IdentityContention)
            .collect(),
    };
    let bytes = serde_json::to_vec(&file)
        .map_err(|e| StoreError::Other(format!("serialize keyspace: {e}")))?;

    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    fs::write(path, bytes)?;
    Ok(())
}

pub fn load(store: &mut PetgraphStore, keyspace: &Keyspace, path: &Path) -> Result<(), StoreError> {
    let bytes = fs::read(path)?;
    let file: KeyspaceFile = serde_json::from_slice(&bytes)
        .map_err(|e| StoreError::Other(format!("parse keyspace file: {e}")))?;

    if !SchemaVersion::CURRENT.can_read(&file.schema_version) {
        return Err(StoreError::SchemaMismatch {
            reader: SchemaVersion::CURRENT,
            graph: file.schema_version,
        });
    }

    store
        .keyspace_mut(keyspace)
        .ingest_warnings
        .extend(file.contention_warnings);
    store.ingest_nodes(keyspace, file.nodes)?;
    store.ingest_edges(keyspace, file.edges)?;
    Ok(())
}

//! Persistence — save/load a keyspace to a single JSON file on disk.
//!
//! RFC §12.1 calls the store file "a cache, not a fixture" — JSON is the
//! canonical format because determinism (G1) is asserted on it. Saving and
//! loading the same keyspace MUST produce byte-identical bytes across calls
//! on unchanged state.
//!
//! File format (one JSON object per file):
//!
//! ```json
//! {
//!   "schema_version": "0.1.0",
//!   "nodes": [ { "id": "...", "label": "...", "props": {...} }, ... ],
//!   "edges": [ { "src": "...", "dst": "...", "label": "...", "props": {...} }, ... ]
//! }
//! ```
//!
//! Nodes are serialized in their stable label/id sort order; edges in their
//! stable src/dst/label order. `BTreeMap`-backed `Props` maps give per-node
//! and per-edge determinism by iteration order.

use std::fs;
use std::path::Path;

use cfdb_core::fact::{Edge, Node};
use cfdb_core::schema::{Keyspace, SchemaVersion};
use cfdb_core::store::StoreBackend;
use cfdb_core::store::StoreError;
use serde::{Deserialize, Serialize};

use crate::PetgraphStore;

/// On-disk representation of one keyspace. Serialized as pretty JSON so
/// humans can diff it against the canonical dump if needed.
#[derive(Debug, Serialize, Deserialize)]
pub struct KeyspaceFile {
    pub schema_version: SchemaVersion,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
}

/// Write a keyspace to `path` as JSON.
///
/// The output is stable across runs: nodes are sorted by
/// `(label, id)` and edges by `(src, dst, label)` before writing.
pub fn save(store: &PetgraphStore, keyspace: &Keyspace, path: &Path) -> Result<(), StoreError> {
    let (mut nodes, mut edges) = store.export(keyspace)?;
    nodes.sort_by(|a, b| a.sort_key().cmp(&b.sort_key()));
    edges.sort_by(|a, b| a.sort_key().cmp(&b.sort_key()));

    // Unknown keyspaces default to V0_1_0 — the pre-`visibility` wire shape.
    // Upgrading a legacy file to V0_1_1 is a no-op for reads: the new
    // `:Item.visibility` attribute is optional on every item, so readers at
    // V0_1_1 treat its absence as "not recorded" rather than malformed.
    let schema_version = store
        .schema_version(keyspace)
        .unwrap_or(SchemaVersion::V0_1_0);
    let file = KeyspaceFile {
        schema_version,
        nodes,
        edges,
    };
    let bytes = serde_json::to_vec_pretty(&file)
        .map_err(|e| StoreError::Other(format!("serialize keyspace: {e}")))?;

    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    fs::write(path, bytes)?;
    Ok(())
}

/// Read a keyspace from `path` and ingest it into `store` under `keyspace`.
///
/// Existing nodes and edges in that keyspace are preserved (additive) — the
/// caller is responsible for `drop_keyspace` first if a clean reload is
/// desired.
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

    store.ingest_nodes(keyspace, file.nodes)?;
    store.ingest_edges(keyspace, file.edges)?;
    Ok(())
}

#![allow(unknown_lints)]
#![deny(non_exhaustive_omitted_patterns)]

mod canonical_dump;
mod graph;
mod graph_view_backend;
pub mod index;
mod ingest_contention;
pub mod persist;

#[cfg(test)]
mod graph_round_trip_tests;
#[cfg(test)]
mod with_indexes_tests;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use cfdb_core::fact::{Edge, Node};
use cfdb_core::result::Warning;
use cfdb_core::schema::{Keyspace, SchemaVersion};
use cfdb_core::store::{StoreBackend, StoreError};
use petgraph::visit::IntoEdgeReferences;

use crate::canonical_dump::canonical_dump;
use crate::graph::KeyspaceState;
use crate::index::spec::IndexSpec;

pub struct PetgraphStore {
    pub(crate) keyspaces: BTreeMap<Keyspace, KeyspaceState>,
    pub(crate) schema_version: SchemaVersion,
    pub(crate) workspace_root: Option<PathBuf>,

    pub(crate) index_spec: IndexSpec,
}

impl Default for PetgraphStore {
    fn default() -> Self {
        Self::new()
    }
}

impl PetgraphStore {
    pub fn new() -> Self {
        Self {
            keyspaces: BTreeMap::new(),
            schema_version: SchemaVersion::CURRENT,
            workspace_root: None,
            index_spec: IndexSpec::empty(),
        }
    }

    pub fn with_workspace(mut self, root: impl Into<PathBuf>) -> Self {
        self.workspace_root = Some(root.into());
        self
    }

    pub fn with_indexes(mut self, spec: IndexSpec) -> Self {
        self.index_spec = spec;
        self
    }

    pub fn workspace_root(&self) -> Option<&Path> {
        self.workspace_root.as_deref()
    }

    pub fn index_spec(&self) -> &IndexSpec {
        &self.index_spec
    }

    fn keyspace_mut(&mut self, keyspace: &Keyspace) -> &mut KeyspaceState {
        if !self.keyspaces.contains_key(keyspace) {
            let spec = self.index_spec.clone();
            self.keyspaces
                .insert(keyspace.clone(), KeyspaceState::new_with_spec(spec));
        }
        self.keyspaces
            .get_mut(keyspace)
            .expect("keyspace just inserted must be present")
    }

    pub fn has_node(&self, keyspace: &Keyspace, id: &str) -> bool {
        self.keyspaces
            .get(keyspace)
            .is_some_and(|state| state.id_to_idx.contains_key(id))
    }

    pub fn export(&self, keyspace: &Keyspace) -> Result<(Vec<Node>, Vec<Edge>), StoreError> {
        let state = self
            .keyspaces
            .get(keyspace)
            .ok_or_else(|| StoreError::UnknownKeyspace(keyspace.clone()))?;

        let nodes: Vec<Node> = state.graph.node_weights().cloned().collect();
        let edges: Vec<Edge> = IntoEdgeReferences::edge_references(&state.graph)
            .map(|e| e.weight().clone())
            .collect();
        Ok((nodes, edges))
    }

    #[must_use]
    pub fn ingest_warnings(&self, keyspace: &Keyspace) -> Vec<Warning> {
        self.keyspaces
            .get(keyspace)
            .map(|s| s.materialized_ingest_warnings())
            .unwrap_or_default()
    }
}

impl StoreBackend for PetgraphStore {
    fn ingest_nodes(&mut self, keyspace: &Keyspace, nodes: Vec<Node>) -> Result<(), StoreError> {
        self.keyspace_mut(keyspace).ingest_nodes(nodes);
        Ok(())
    }

    fn ingest_edges(&mut self, keyspace: &Keyspace, edges: Vec<Edge>) -> Result<(), StoreError> {
        self.keyspace_mut(keyspace).ingest_edges(edges);
        Ok(())
    }

    fn schema_version(&self, keyspace: &Keyspace) -> Result<SchemaVersion, StoreError> {
        if !self.keyspaces.contains_key(keyspace) {
            return Err(StoreError::UnknownKeyspace(keyspace.clone()));
        }
        Ok(self.schema_version)
    }

    fn list_keyspaces(&self) -> Vec<Keyspace> {
        self.keyspaces.keys().cloned().collect()
    }

    fn drop_keyspace(&mut self, keyspace: &Keyspace) -> Result<(), StoreError> {
        self.keyspaces.remove(keyspace);
        Ok(())
    }

    fn canonical_dump(&self, keyspace: &Keyspace) -> Result<String, StoreError> {
        let state = self
            .keyspaces
            .get(keyspace)
            .ok_or_else(|| StoreError::UnknownKeyspace(keyspace.clone()))?;
        Ok(canonical_dump(state))
    }
}

#[cfg(test)]
mod tests;

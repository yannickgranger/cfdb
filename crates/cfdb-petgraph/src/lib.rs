//! cfdb-petgraph — `StoreBackend` implementation on `petgraph::StableDiGraph`.
//!
//! One `KeyspaceState` per keyspace; each holds a `StableDiGraph<Node, Edge>`
//! plus an insertion-ordered id → `NodeIndex` map (`indexmap::IndexMap`) and a
//! `BTreeMap`-based label index.
//!
//! Evaluation is routed through `eval::Evaluator` which ports the Gate 3 spike
//! (`studies/spike/petgraph/src/main.rs`) onto the real
//! `cfdb_core::Query` AST. Canonical dumping is a single sorted `Vec<String>`
//! join so two consecutive calls are byte-identical (RFC §12 G1).
//!
//! NOTE on pathological-shape lint (study 001 §4.2): v0.1 delegates that check
//! to `cfdb-query::shape_lint` — callers run the lint at parse time and
//! decide whether to call `execute`. The evaluator does not re-run the lint.

mod canonical_dump;
mod enrich;
mod enrich_backend;
mod eval;
mod graph;
pub mod index;
pub mod persist;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use cfdb_core::fact::{Edge, Node};
use cfdb_core::query::Query;
use cfdb_core::result::QueryResult;
use cfdb_core::schema::{Keyspace, SchemaVersion};
use cfdb_core::store::{StoreBackend, StoreError};
use petgraph::visit::IntoEdgeReferences;

use crate::canonical_dump::canonical_dump;
use crate::eval::Evaluator;
use crate::graph::KeyspaceState;

/// In-memory petgraph-backed store. One `StableDiGraph` per keyspace.
///
/// The store is `Send + Sync` by virtue of its contents; concurrent readers
/// are not yet supported — the trait takes `&mut self` for writes and `&self`
/// for reads, so callers wrap the store in an external `RwLock` if they need
/// parallel evaluation.
pub struct PetgraphStore {
    pub(crate) keyspaces: BTreeMap<Keyspace, KeyspaceState>,
    pub(crate) schema_version: SchemaVersion,
    /// Optional workspace root for enrichment passes that read files
    /// (`enrich_rfc_docs`, `enrich_concepts`). `None` when the store was
    /// constructed for tests or for non-enrichment workflows. Wired by
    /// [`crate::PetgraphStore::with_workspace`]; [`crate::PetgraphStore::new`]
    /// remains argument-less so existing callers (30+ test sites, persist
    /// round-trips) compile unchanged. Slices 43-D (issue #107) and 43-F
    /// (issue #109) will consume this field via
    /// [`crate::PetgraphStore::workspace_root`] without changing the
    /// `EnrichBackend` port signature — clean-arch B4 resolution
    /// (`council/43/clean-arch.md`).
    pub(crate) workspace_root: Option<PathBuf>,
}

impl Default for PetgraphStore {
    fn default() -> Self {
        Self::new()
    }
}

impl PetgraphStore {
    /// Create an empty store at `SchemaVersion::CURRENT`. New keyspaces
    /// are tagged with the current build's schema version; any legacy file
    /// ingested via `persist::load` retains its own version unless it is
    /// rewritten through `persist::save` (which stamps CURRENT).
    pub fn new() -> Self {
        Self {
            keyspaces: BTreeMap::new(),
            schema_version: SchemaVersion::CURRENT,
            workspace_root: None,
        }
    }

    /// Attach a workspace root for enrichment passes that read files.
    /// Builder-style — returns `self` so a caller can chain
    /// `PetgraphStore::new().with_workspace(path)` without changing the
    /// zero-arg `::new()` signature that 30+ call sites depend on. The
    /// composition root (`cfdb-cli::compose::load_store`) will wire this
    /// when slices 43-D / 43-F actually need a workspace path; until then
    /// every existing construction path returns `workspace_root = None`.
    pub fn with_workspace(mut self, root: impl Into<PathBuf>) -> Self {
        self.workspace_root = Some(root.into());
        self
    }

    /// Return the attached workspace root, if any. Slices 43-D and 43-F
    /// will consume this to locate `docs/rfc/*.md` and
    /// `.cfdb/concepts/*.toml` without modifying the `EnrichBackend` port
    /// signature.
    pub fn workspace_root(&self) -> Option<&Path> {
        self.workspace_root.as_deref()
    }

    /// Return a reference to a keyspace, creating it if missing.
    fn keyspace_mut(&mut self, keyspace: &Keyspace) -> &mut KeyspaceState {
        if !self.keyspaces.contains_key(keyspace) {
            self.keyspaces
                .insert(keyspace.clone(), KeyspaceState::new());
        }
        self.keyspaces
            .get_mut(keyspace)
            .expect("keyspace just inserted must be present")
    }

    /// Export the raw nodes and edges of a keyspace. Used by
    /// [`crate::persist::save`] to serialize the keyspace to disk. Returns
    /// facts in insertion order; the caller sorts for canonical output.
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

    fn execute(&self, keyspace: &Keyspace, query: &Query) -> Result<QueryResult, StoreError> {
        let state = self
            .keyspaces
            .get(keyspace)
            .ok_or_else(|| StoreError::UnknownKeyspace(keyspace.clone()))?;
        let mut result = Evaluator::new(state, &query.params).run(query);
        let mut prepended = state.ingest_warnings.clone();
        prepended.append(&mut result.warnings);
        result.warnings = prepended;
        Ok(result)
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

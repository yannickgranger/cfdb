//! cfdb-petgraph — `StoreBackend` implementation on `petgraph::StableDiGraph`.
//!
//! One `KeyspaceState` per keyspace; each holds a `StableDiGraph<Node, Edge>`
//! plus an insertion-ordered id → `NodeIndex` map (`indexmap::IndexMap`) and a
//! `BTreeMap`-based label index.
//!
//! Evaluation is routed through `eval::Evaluator` which ports the Gate 3 spike
//! (`studies/spike/petgraph/src/main.rs`) onto the real
//! `cfdb_core::Query` AST. Canonical dumping is a single sorted `Vec<String>`
//! join so two consecutive calls are byte-identical.
//!
//! NOTE on pathological-shape lint: v0.1 delegates that check to
//! `cfdb-query::shape_lint` — callers run the lint at parse time and
//! decide whether to call `execute`. The evaluator does not re-run the lint.
//!
//! cfdb-core schema enums are `#[non_exhaustive]`. Cross-crate `match` sites
//! on those enums require a `_ =>` arm by hard compile error (E0004). The
//! `non_exhaustive_omitted_patterns` lint further tightens this at the
//! wildcard-arm boundary; we deny it preemptively so the attribute
//! auto-activates when the lint stabilises (currently nightly-only on rust
//! 1.93; `allow(unknown_lints)` keeps the attribute inert on stable).

#![allow(unknown_lints)]
#![deny(non_exhaustive_omitted_patterns)]

mod canonical_dump;
mod enrich;
mod enrich_backend;
mod eval;
pub mod explain;
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
use cfdb_core::query::Query;
use cfdb_core::result::{QueryResult, Warning};
use cfdb_core::schema::{Keyspace, SchemaVersion};
use cfdb_core::store::{StoreBackend, StoreError};
use petgraph::visit::IntoEdgeReferences;

use crate::canonical_dump::canonical_dump;
use crate::eval::Evaluator;
use crate::graph::KeyspaceState;
use crate::index::spec::IndexSpec;

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
    /// round-trips) compile unchanged.
    pub(crate) workspace_root: Option<PathBuf>,

    /// Index spec carried at the store level. Each newly-created
    /// [`KeyspaceState`] is bound to this spec via
    /// [`KeyspaceState::new_with_spec`] so per-keyspace `by_prop` gets
    /// populated on ingest (the composition-root ships one `IndexSpec` that
    /// flows to every keyspace the store owns). Empty by default — existing
    /// callers get identical behaviour.
    pub(crate) index_spec: IndexSpec,
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
            index_spec: IndexSpec::empty(),
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

    /// Attach an [`IndexSpec`]. Every [`KeyspaceState`] the store creates
    /// from this point on is bound to `spec`, so `ingest_nodes` /
    /// `persist::load` populate per-keyspace `by_prop` posting lists.
    /// Symmetric to [`Self::with_workspace`]; chain after `new` at the
    /// composition root. RFC-035 §3.8.
    pub fn with_indexes(mut self, spec: IndexSpec) -> Self {
        self.index_spec = spec;
        self
    }

    /// Return the attached workspace root, if any. Slices 43-D and 43-F
    /// will consume this to locate `docs/rfc/*.md` and
    /// `.cfdb/concepts/*.toml` without modifying the `EnrichBackend` port
    /// signature.
    pub fn workspace_root(&self) -> Option<&Path> {
        self.workspace_root.as_deref()
    }

    /// Return a reference to the store-level [`IndexSpec`]. Mirrors
    /// [`Self::workspace_root`] — lets the composition root and test
    /// harnesses inspect what `with_indexes` received without widening
    /// the mutation surface. RFC-035 §3.8.
    pub fn index_spec(&self) -> &IndexSpec {
        &self.index_spec
    }

    /// Return a reference to a keyspace, creating it if missing. New
    /// keyspaces inherit the store's [`Self::index_spec`] so on-ingest
    /// `by_prop` population is active from the first node (RFC-035 §3.8).
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

    /// Does the given keyspace contain a node with this id? Returns
    /// `false` if the keyspace doesn't exist yet — symmetric to
    /// [`ingest_one_edge`]'s "unknown dst id ⇒ drop" treatment.
    ///
    /// Consumed by [`cfdb_hir_petgraph_adapter::PetgraphAdapter`] (issue
    /// #388) to decide whether to synthesize a stub `:Item` for a
    /// foreign-callee CALLS edge dst before ingest. Pure read; no
    /// behavioral change to existing ingest semantics.
    ///
    /// [`ingest_one_edge`]: crate::graph::KeyspaceState
    pub fn has_node(&self, keyspace: &Keyspace, id: &str) -> bool {
        self.keyspaces
            .get(keyspace)
            .is_some_and(|state| state.id_to_idx.contains_key(id))
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

    /// Slice-7 (#186) concrete sibling of [`StoreBackend::execute`].
    /// Returns both the `QueryResult` and a trace of
    /// [`crate::explain::ExplainRow`] describing how each
    /// `candidate_nodes` invocation was satisfied (indexed fast path vs
    /// full-scan fallback). NOT on `StoreBackend` — the index
    /// observability surface stays internal to `cfdb-petgraph` per
    /// RFC-035 §4 (StoreBackend trait is untouched).
    pub fn execute_explained(
        &self,
        keyspace: &Keyspace,
        query: &Query,
    ) -> Result<(QueryResult, Vec<crate::explain::ExplainRow>), StoreError> {
        let state = self
            .keyspaces
            .get(keyspace)
            .ok_or_else(|| StoreError::UnknownKeyspace(keyspace.clone()))?;
        let (mut result, explain) =
            Evaluator::new_with_explain(state, &query.params).run_explained(query);
        let mut prepended = state.materialized_ingest_warnings();
        prepended.append(&mut result.warnings);
        result.warnings = prepended;
        Ok((result, explain))
    }

    /// Ingest-time diagnostics for one keyspace (RFC-054 §3.4, 54-A #556) —
    /// recorded warnings plus the over-cap summary row. Deliberately an
    /// inherent method, NOT on [`StoreBackend`] (RFC-035 §4
    /// `execute_explained` precedent); an unknown keyspace yields empty.
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

    fn execute(&self, keyspace: &Keyspace, query: &Query) -> Result<QueryResult, StoreError> {
        let state = self
            .keyspaces
            .get(keyspace)
            .ok_or_else(|| StoreError::UnknownKeyspace(keyspace.clone()))?;
        let mut result = Evaluator::new(state, &query.params).run(query);
        let mut prepended = state.materialized_ingest_warnings();
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

//! StoreBackend trait — storage and keyspace-lifecycle surface;
//! QueryBackend trait — the query-execution contract.
//!
//! Every storage layer implements `StoreBackend` (six methods). The
//! cfdb-query parser/builder constructs `Query` AST values; consumers then
//! call `execute(&keyspace, &query)` on a `QueryBackend`, whose implementor
//! (`cfdb-eval`'s `QueryEngine`) reads the keyspace through a `GraphReader`
//! resolved from the store's `GraphBackend`. Both traits are deliberately
//! small and hide all backend-specific state behind `&self`.
//!
//! Enrichment (docs / metrics / history / concepts) was previously bolted on
//! as default-stub methods here; it is now a sibling trait
//! [`crate::enrich::EnrichBackend`]. Library consumers that only query should
//! not pull the enrichment surface.

use thiserror::Error;

use crate::fact::{Edge, Node};
use crate::query::Query;
use crate::result::QueryResult;
use crate::schema::{Keyspace, SchemaVersion};

/// Errors produced by a backend during ingest, evaluation, or snapshot ops.
/// Intentionally small — parser errors live in cfdb-query, not here.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum StoreError {
    #[error("keyspace not found: {0}")]
    UnknownKeyspace(Keyspace),

    #[error("schema mismatch: reader={reader}, graph={graph}")]
    SchemaMismatch {
        reader: SchemaVersion,
        graph: SchemaVersion,
    },

    #[error("evaluation error: {0}")]
    Eval(String),

    #[error("ingest error: {0}")]
    Ingest(String),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error("{0}")]
    Other(String),
}

/// The storage surface: ingest, keyspace lifecycle, canonical dump.
///
/// # Determinism guarantees
///
/// Implementors MUST uphold:
/// - **G1**: same input facts + same schema version → byte-identical canonical
///   dump. Evaluation may parallelize but output ordering is stable.
/// - **G2**: query evaluation is read-only. No query may mutate the graph —
///   the evaluator reaches a keyspace only through the `&self`-only
///   [`crate::graph::GraphReader`], so this holds by construction.
/// - **G3**: `ingest_*` is additive-or-replace. A single `ingest_nodes` call
///   never deletes a node that was not in the current batch.
/// - **G4**: `schema_version` is monotonic within a major.
/// - **G5**: snapshots are immutable; `drop_keyspace` is the only deletion
///   verb exposed.
pub trait StoreBackend: Send + Sync {
    /// Bulk-ingest a batch of nodes into a keyspace. Creates the keyspace if
    /// it does not exist.
    fn ingest_nodes(&mut self, keyspace: &Keyspace, nodes: Vec<Node>) -> Result<(), StoreError>;

    /// Bulk-ingest a batch of edges. Node ids must already exist; unknown ids
    /// are reported as a warning rather than an error so bulk loads from
    /// partially-extracted sources degrade gracefully.
    fn ingest_edges(&mut self, keyspace: &Keyspace, edges: Vec<Edge>) -> Result<(), StoreError>;

    /// Current schema version for the keyspace.
    fn schema_version(&self, keyspace: &Keyspace) -> Result<SchemaVersion, StoreError>;

    /// List keyspaces currently known to this backend.
    fn list_keyspaces(&self) -> Vec<Keyspace>;

    /// Drop a keyspace entirely. G5: this is the only deletion verb.
    fn drop_keyspace(&mut self, keyspace: &Keyspace) -> Result<(), StoreError>;

    /// Produce the canonical sorted dump of a keyspace (JSONL). G1 hinges on
    /// this being byte-stable across runs with the same inputs.
    fn canonical_dump(&self, keyspace: &Keyspace) -> Result<String, StoreError>;
}

/// The query-execution contract: evaluate a parsed [`Query`] against one
/// keyspace and return its rows and warnings. Read-only (G2) — the
/// implementor evaluates over a [`crate::graph::GraphReader`] and cannot
/// mutate the keyspace it reads.
///
/// `Send + Sync` mirrors [`StoreBackend`]/[`crate::enrich::EnrichBackend`]
/// so a generic engine over a `GraphBackend` can itself be `Send + Sync`.
pub trait QueryBackend: Send + Sync {
    /// Evaluate a parsed Query against the given keyspace. Read-only (G2).
    /// `Err(StoreError::UnknownKeyspace)` if the store has never seen this
    /// keyspace.
    fn execute(&self, keyspace: &Keyspace, query: &Query) -> Result<QueryResult, StoreError>;
}

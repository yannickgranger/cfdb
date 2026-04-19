//! StoreBackend trait — the single evaluation entry point.
//!
//! Every storage layer (cfdb-petgraph for v0.1, future alternatives) implements
//! this trait. The cfdb-query parser/builder constructs `Query` AST values;
//! consumers then call `backend.execute(&query)`. The trait is deliberately
//! small (four methods) and hides all backend-specific state behind `&self`.

use thiserror::Error;

use crate::enrich::EnrichReport;
use crate::fact::{Edge, Node};
use crate::query::Query;
use crate::result::QueryResult;
use crate::schema::{Keyspace, SchemaVersion};

/// Errors produced by a backend during ingest, evaluation, or snapshot ops.
/// Intentionally small — parser errors live in cfdb-query, not here.
#[derive(Debug, Error)]
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

/// The storage and query evaluation surface.
///
/// # Determinism (RFC §6)
///
/// Implementors MUST uphold:
/// - **G1**: same input facts + same schema version → byte-identical canonical
///   dump. Evaluation may parallelize but output ordering is stable.
/// - **G2**: `execute` is read-only. No query may mutate the graph. This is a
///   trait invariant, not merely a convention.
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

    /// Evaluate a parsed Query against the given keyspace. Read-only (G2).
    fn execute(&self, keyspace: &Keyspace, query: &Query) -> Result<QueryResult, StoreError>;

    /// Current schema version for the keyspace.
    fn schema_version(&self, keyspace: &Keyspace) -> Result<SchemaVersion, StoreError>;

    /// List keyspaces currently known to this backend.
    fn list_keyspaces(&self) -> Vec<Keyspace>;

    /// Drop a keyspace entirely. G5: this is the only deletion verb.
    fn drop_keyspace(&mut self, keyspace: &Keyspace) -> Result<(), StoreError>;

    /// Produce the canonical sorted dump of a keyspace (JSONL). G1 hinges on
    /// this being byte-stable across runs with the same inputs.
    fn canonical_dump(&self, keyspace: &Keyspace) -> Result<String, StoreError>;

    /// Enrich a keyspace with documentation facts (rustdoc, README, RFC text).
    ///
    /// **Phase A stub (#3628):** the default implementation returns
    /// [`EnrichReport::not_implemented`]. Real implementations ship in v0.2 /
    /// Phase D per EPIC #3622. Backends override this method as the
    /// enrichment passes land.
    fn enrich_docs(&mut self, _keyspace: &Keyspace) -> Result<EnrichReport, StoreError> {
        Ok(EnrichReport::not_implemented("enrich_docs"))
    }

    /// Enrich a keyspace with quality-signal facts (complexity, unwrap counts,
    /// clone-in-loop counts, etc.).
    ///
    /// **Phase A stub (#3628):** see [`Self::enrich_docs`].
    fn enrich_metrics(&mut self, _keyspace: &Keyspace) -> Result<EnrichReport, StoreError> {
        Ok(EnrichReport::not_implemented("enrich_metrics"))
    }

    /// Enrich a keyspace with git-history facts (last-touched, churn, author).
    ///
    /// **Phase A stub (#3628):** see [`Self::enrich_docs`].
    fn enrich_history(&mut self, _keyspace: &Keyspace) -> Result<EnrichReport, StoreError> {
        Ok(EnrichReport::not_implemented("enrich_history"))
    }

    /// Enrich a keyspace with bounded-context / concept facts (which crate
    /// owns which type, which concepts are canonical bypasses).
    ///
    /// **Phase A stub (#3628):** see [`Self::enrich_docs`].
    fn enrich_concepts(&mut self, _keyspace: &Keyspace) -> Result<EnrichReport, StoreError> {
        Ok(EnrichReport::not_implemented("enrich_concepts"))
    }
}

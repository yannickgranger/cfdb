use thiserror::Error;

use crate::fact::{Edge, Node};
use crate::query::Query;
use crate::result::QueryResult;
use crate::schema::{Keyspace, SchemaVersion};

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

pub trait StoreBackend: Send + Sync {
    fn ingest_nodes(&mut self, keyspace: &Keyspace, nodes: Vec<Node>) -> Result<(), StoreError>;

    fn ingest_edges(&mut self, keyspace: &Keyspace, edges: Vec<Edge>) -> Result<(), StoreError>;

    fn schema_version(&self, keyspace: &Keyspace) -> Result<SchemaVersion, StoreError>;

    fn list_keyspaces(&self) -> Vec<Keyspace>;

    fn drop_keyspace(&mut self, keyspace: &Keyspace) -> Result<(), StoreError>;

    fn canonical_dump(&self, keyspace: &Keyspace) -> Result<String, StoreError>;
}

pub trait QueryBackend: Send + Sync {
    fn execute(&self, keyspace: &Keyspace, query: &Query) -> Result<QueryResult, StoreError>;
}

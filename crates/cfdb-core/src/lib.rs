//! cfdb-core — foundation types and traits for the cfdb graph store.
//!
//! This crate is the hub of the cfdb workspace. Every other crate (cfdb-query,
//! cfdb-petgraph, cfdb-extractor, cfdb-cli) depends on it. cfdb-core itself has
//! **zero dependencies** on the parser, the store, the extractor, or the wire
//! forms — the dependency rule points inward (Clean Architecture, RFC §8).
//!
//! The six public modules:
//! - [`fact`]: Node, Edge, PropValue — the wire format for a single fact.
//! - [`schema`]: Label, EdgeLabel, SchemaVersion, SchemaDescribe — RFC §7 schema.
//! - [`query`]: Query AST, Pattern, Predicate, Aggregation, Param — the
//!   interchange format between parser and evaluator.
//! - [`result`]: QueryResult, Row, Warning — the shape returned to callers.
//! - [`enrich`]: EnrichBackend trait and EnrichReport — the four `enrich_*`
//!   verbs live here, split from StoreBackend per RFC-031 §2 (Phase A stubs
//!   in v0.1, full implementations in v0.2 / Phase D).
//! - [`store`]: StoreBackend trait — storage, query evaluation, and lifecycle.
//!
//! Determinism invariants G1–G5 (RFC §6) are enforced at the trait level where
//! possible and documented where they must be respected by implementors.

pub mod cfg_gate;
pub mod enrich;
pub mod fact;
pub mod query;
pub mod result;
pub mod schema;
pub mod store;
pub mod visibility;

pub use cfg_gate::CfgGate;
pub use enrich::{EnrichBackend, EnrichReport};
pub use fact::{Edge, Node, PropValue, Props};
pub use query::{
    Aggregation, CompareOp, Direction, EdgePattern, Expr, ItemKind, NodePattern, OrderBy, Param,
    PathPattern, Pattern, Predicate, Projection, ProjectionValue, Query, ReturnClause,
    UnknownItemKind, WithClause,
};
pub use result::{QueryResult, Row, RowValue, Warning, WarningKind};
pub use schema::{
    schema_describe, AttributeDescriptor, EdgeLabel, EdgeLabelDescriptor, Keyspace, Label,
    NodeLabelDescriptor, Provenance, SchemaDescribe, SchemaVersion,
};
pub use store::{StoreBackend, StoreError};
pub use visibility::Visibility;

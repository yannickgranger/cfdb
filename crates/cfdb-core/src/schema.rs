//! Schema types — node labels, edge labels, keyspaces, schema version, and
//! the self-documenting `schema_describe()` contract.
//!
//! RFC §7 defines the ten node labels and ~20 edge labels. This module encodes
//! them as plain strings wrapped in newtypes so the extractor, parser, and
//! evaluator can share a single vocabulary without stringly-typing it.
//!
//! Internal layout (private submodules, re-exported here so external callers
//! see the flat `schema::*` surface documented in RFC §7):
//!
//! - [`labels`] — [`Label`], [`EdgeLabel`], [`Keyspace`], [`SchemaVersion`].
//! - [`descriptors`] — [`Provenance`], [`AttributeDescriptor`],
//!   [`NodeLabelDescriptor`], [`EdgeLabelDescriptor`], [`SchemaDescribe`].
//! - [`describe`] — [`schema_describe`], the runtime contract (RFC §6A.1).

mod describe;
mod descriptors;
mod labels;

pub use describe::schema_describe;
pub use descriptors::{
    AttributeDescriptor, EdgeLabelDescriptor, NodeLabelDescriptor, Provenance, SchemaDescribe,
};
pub use labels::{EdgeLabel, Keyspace, Label, SchemaVersion};

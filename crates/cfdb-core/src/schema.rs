//! Schema types — node labels, edge labels, keyspaces, schema version, and
//! the self-documenting `schema_describe()` contract.
//!
//! This module defines the ten node labels and ~20 edge labels as plain
//! strings wrapped in newtypes so the extractor, parser, and evaluator can
//! share a single vocabulary without stringly-typing it.
//!
//! Internal layout (private submodules, re-exported here so external callers
//! see the flat `schema::*` surface):
//!
//! - [`labels`] — [`Label`], [`EdgeLabel`], [`Keyspace`], [`SchemaVersion`].
//! - [`descriptors`] — [`Provenance`], [`AttributeDescriptor`],
//!   [`NodeLabelDescriptor`], [`EdgeLabelDescriptor`], [`SchemaDescribe`].
//! - [`describe`] — [`schema_describe`], the runtime contract.

mod describe;
mod descriptors;
mod labels;
mod version;

pub use describe::schema_describe;
pub use descriptors::{
    AttributeDescriptor, EdgeLabelDescriptor, NodeLabelDescriptor, Provenance, SchemaDescribe,
};
pub use labels::{EdgeLabel, Keyspace, Label, RECEIVER_POSITION};
pub use version::SchemaVersion;

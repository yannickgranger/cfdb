//! The `schema_describe()` verb — runtime contract for the cfdb vocabulary.
//!
//! RFC §6A.1 / PLAN-v1 §6.1. Deterministic and byte-stable per build (G1).

use super::descriptors::{NodeLabelDescriptor, SchemaDescribe};
use super::labels::SchemaVersion;

mod edges;
mod nodes;

#[cfg(test)]
mod tests;

/// Return the canonical schema description for the current cfdb-core build.
///
/// This is the runtime contract cfdb exposes to consumers — the complete
/// vocabulary of node labels, edge labels, attributes, and per-attribute
/// provenance (RFC §7 fact schema, PLAN-v1 §6.1). Deterministic and
/// byte-stable for a given build.
pub fn schema_describe() -> SchemaDescribe {
    SchemaDescribe {
        schema_version: SchemaVersion::CURRENT,
        nodes: node_descriptors(),
        edges: edges::edge_descriptors(),
    }
}

fn node_descriptors() -> Vec<NodeLabelDescriptor> {
    vec![
        nodes::crate_node_descriptor(),
        nodes::module_node_descriptor(),
        nodes::file_node_descriptor(),
        nodes::item_node_descriptor(),
        nodes::field_node_descriptor(),
        nodes::variant_node_descriptor(),
        nodes::param_node_descriptor(),
        nodes::call_site_node_descriptor(),
        nodes::entry_point_node_descriptor(),
        nodes::concept_node_descriptor(),
        nodes::context_node_descriptor(),
        nodes::rfc_doc_node_descriptor(),
    ]
}

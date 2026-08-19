use super::descriptors::{NodeLabelDescriptor, SchemaDescribe};
use super::version::SchemaVersion;

mod edges;
mod nodes;

#[cfg(test)]
mod tests;

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
        nodes::const_table_node_descriptor(),
        nodes::literal_node_descriptor(),
        nodes::argument_node_descriptor(),
        nodes::match_site_node_descriptor(),
    ]
}

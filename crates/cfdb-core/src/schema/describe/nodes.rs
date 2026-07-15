//! Node-label descriptors for `schema_describe()`.
//!
//! Grouped into cohesive submodules (#467) so each file stays under the
//! 500-line architecture threshold. The split is purely structural:
//! `schema_describe()` aggregates the same descriptor content regardless of
//! source-file layout, so the `FROZEN_NARRATIVE_DIGEST` pin is unchanged.

mod call_graph;
mod overlay;
mod structural;

pub(in crate::schema::describe) use call_graph::{
    argument_node_descriptor, call_site_node_descriptor, entry_point_node_descriptor,
    match_site_node_descriptor,
};
pub(in crate::schema::describe) use overlay::{
    concept_node_descriptor, const_table_node_descriptor, context_node_descriptor,
    literal_node_descriptor, rfc_doc_node_descriptor,
};
pub(in crate::schema::describe) use structural::{
    crate_node_descriptor, field_node_descriptor, file_node_descriptor, item_node_descriptor,
    module_node_descriptor, param_node_descriptor, variant_node_descriptor,
};

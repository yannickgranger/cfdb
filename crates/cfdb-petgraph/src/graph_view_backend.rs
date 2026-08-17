//! `GraphView` for `KeyspaceState`, `GraphBackend` for `PetgraphStore`
//! (RFC-056 slice 056-0).
//!
//! Thin delegation to `KeyspaceState`'s existing `pub(crate)` fields/methods
//! — this file introduces no new behavior, only a `pub` trait surface over
//! what already exists.

use petgraph::visit::EdgeRef;
use petgraph::Direction as PetDirection;

use cfdb_core::fact::{Edge, Node, PropValue};
use cfdb_core::graph::{GraphBackend, GraphView};
use cfdb_core::schema::{Direction, EdgeLabel, Keyspace, Label};
use cfdb_core::store::StoreError;

use crate::graph::KeyspaceState;
use crate::PetgraphStore;

impl GraphView for KeyspaceState {
    fn node_by_id(&self, id: &str) -> Option<&Node> {
        let idx = *self.id_to_idx.get(id)?;
        self.graph.node_weight(idx)
    }

    fn nodes_with_label(&self, label: &Label) -> Vec<String> {
        // Delegates to the existing sorted-NodeIndex accessor, then resolves
        // each index to its id — same order, id-based per the port contract.
        KeyspaceState::nodes_with_label(self, label)
            .into_iter()
            .filter_map(|idx| self.graph.node_weight(idx).map(|n| n.id.clone()))
            .collect()
    }

    fn neighbors(&self, id: &str, dir: Direction) -> Vec<(EdgeLabel, String)> {
        let Some(&idx) = self.id_to_idx.get(id) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let mut walk = |pet_dir: PetDirection| {
            for edge in self.graph.edges_directed(idx, pet_dir) {
                let other = match pet_dir {
                    PetDirection::Outgoing => edge.target(),
                    PetDirection::Incoming => edge.source(),
                };
                if let Some(other_node) = self.graph.node_weight(other) {
                    out.push((edge.weight().label.clone(), other_node.id.clone()));
                }
            }
        };
        match dir {
            Direction::Out => walk(PetDirection::Outgoing),
            Direction::In => walk(PetDirection::Incoming),
            Direction::Undirected => {
                walk(PetDirection::Outgoing);
                walk(PetDirection::Incoming);
            }
        }
        out
    }

    fn set_attr(&mut self, id: &str, key: &str, value: PropValue) -> bool {
        let Some(&idx) = self.id_to_idx.get(id) else {
            return false;
        };
        let Some(node) = self.graph.node_weight_mut(idx) else {
            return false;
        };
        node.props.insert(key.to_string(), value);
        true
    }

    fn ingest_nodes(&mut self, nodes: Vec<Node>) {
        KeyspaceState::ingest_nodes(self, nodes)
    }

    fn ingest_edges(&mut self, edges: Vec<Edge>) {
        KeyspaceState::ingest_edges(self, edges)
    }
}

impl GraphBackend for PetgraphStore {
    fn graph_view(&mut self, keyspace: &Keyspace) -> Result<&mut dyn GraphView, StoreError> {
        let state = self
            .keyspaces
            .get_mut(keyspace)
            .ok_or_else(|| StoreError::UnknownKeyspace(keyspace.clone()))?;
        Ok(state as &mut dyn GraphView)
    }

    fn workspace_root(&self) -> Option<&std::path::Path> {
        PetgraphStore::workspace_root(self)
    }
}

#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::path::Path;

use petgraph::stable_graph::{EdgeIndex, NodeIndex};
use petgraph::visit::EdgeRef;
use petgraph::Direction as PetDirection;

use cfdb_core::fact::{Edge, Node, PropValue};
use cfdb_core::graph::{EdgeHandle, GraphBackend, GraphReader, GraphView, NodeHandle};
use cfdb_core::query::{NodePattern, ParamBinding, Predicate};
use cfdb_core::result::Warning;
use cfdb_core::schema::{Direction, EdgeLabel, Keyspace, Label};
use cfdb_core::store::StoreError;

use crate::graph::KeyspaceState;
use crate::index::build::index_key_of;
use crate::index::lookup::candidates_from_index;
use crate::PetgraphStore;

fn node_handle(idx: NodeIndex) -> NodeHandle {
    NodeHandle::from_raw(idx.index() as u32)
}

fn edge_handle(idx: EdgeIndex) -> EdgeHandle {
    EdgeHandle::from_raw(idx.index() as u32)
}

fn node_index(h: NodeHandle) -> NodeIndex {
    NodeIndex::new(h.raw() as usize)
}

fn edge_index(h: EdgeHandle) -> EdgeIndex {
    EdgeIndex::new(h.raw() as usize)
}

impl GraphView for KeyspaceState {
    fn node_by_id(&self, id: &str) -> Option<&Node> {
        let idx = *self.id_to_idx.get(id)?;
        self.graph.node_weight(idx)
    }

    fn nodes_with_label(&self, label: &Label) -> Vec<String> {
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

impl GraphReader for KeyspaceState {
    fn has_label(&self, label: &Label) -> bool {
        KeyspaceState::has_label(self, label)
    }

    fn labels(&self) -> Vec<Label> {
        self.by_label.keys().cloned().collect()
    }

    fn has_edge_label(&self, label: &EdgeLabel) -> bool {
        KeyspaceState::has_edge_label(self, label)
    }

    fn edge_labels(&self) -> Vec<EdgeLabel> {
        self.edge_labels.iter().cloned().collect()
    }

    fn nodes_with_label(&self, label: &Label) -> Vec<NodeHandle> {
        KeyspaceState::nodes_with_label(self, label)
            .into_iter()
            .map(node_handle)
            .collect()
    }

    fn all_nodes_sorted(&self) -> Vec<NodeHandle> {
        KeyspaceState::all_nodes_sorted(self)
            .into_iter()
            .map(node_handle)
            .collect()
    }

    fn node(&self, h: NodeHandle) -> Option<&Node> {
        self.graph.node_weight(node_index(h))
    }

    fn edge(&self, h: EdgeHandle) -> Option<&Edge> {
        self.graph.edge_weight(edge_index(h))
    }

    fn edges_out(&self, h: NodeHandle) -> Vec<(EdgeHandle, NodeHandle)> {
        self.graph
            .edges_directed(node_index(h), PetDirection::Outgoing)
            .map(|e| (edge_handle(e.id()), node_handle(e.target())))
            .collect()
    }

    fn edges_in(&self, h: NodeHandle) -> Vec<(EdgeHandle, NodeHandle)> {
        self.graph
            .edges_directed(node_index(h), PetDirection::Incoming)
            .map(|e| (edge_handle(e.id()), node_handle(e.source())))
            .collect()
    }

    fn index_candidates(
        &self,
        np: &NodePattern,
        where_clause: Option<&Predicate>,
        params: &BTreeMap<String, ParamBinding>,
        bound_var_prop: &dyn Fn(&str, &str) -> Option<PropValue>,
    ) -> Option<Vec<NodeHandle>> {
        let bound_key =
            |var: &str, prop: &str| bound_var_prop(var, prop).and_then(|pv| index_key_of(&pv));
        candidates_from_index(self, np, where_clause, params, &bound_key)
            .map(|idxs| idxs.into_iter().map(node_handle).collect())
    }

    fn indexed_prop_is_populated(&self, label: &Label, tag: &str) -> bool {
        self.by_prop
            .get(&(label.clone(), tag.to_string()))
            .is_some_and(|bucket| !bucket.is_empty())
    }

    fn ingest_warnings(&self) -> Vec<Warning> {
        self.materialized_ingest_warnings()
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

    fn graph_reader(&self, keyspace: &Keyspace) -> Result<&dyn GraphReader, StoreError> {
        let state = self
            .keyspaces
            .get(keyspace)
            .ok_or_else(|| StoreError::UnknownKeyspace(keyspace.clone()))?;
        Ok(state as &dyn GraphReader)
    }

    fn workspace_root(&self) -> Option<&Path> {
        PetgraphStore::workspace_root(self)
    }
}

#[cfg(test)]
mod reader_tests;
#[cfg(test)]
mod tests;

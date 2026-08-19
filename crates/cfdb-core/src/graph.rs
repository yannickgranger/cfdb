use std::collections::BTreeMap;
use std::path::Path;

use crate::fact::{Edge, Node, PropValue};
use crate::query::{NodePattern, ParamBinding, Predicate};
use crate::result::Warning;
use crate::schema::{Direction, EdgeLabel, Keyspace, Label};
use crate::store::StoreError;

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeHandle(u32);

impl NodeHandle {
    #[must_use]
    pub const fn from_raw(raw: u32) -> Self {
        Self(raw)
    }

    #[must_use]
    pub const fn raw(self) -> u32 {
        self.0
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EdgeHandle(u32);

impl EdgeHandle {
    #[must_use]
    pub const fn from_raw(raw: u32) -> Self {
        Self(raw)
    }

    #[must_use]
    pub const fn raw(self) -> u32 {
        self.0
    }
}

pub trait GraphView {
    fn node_by_id(&self, id: &str) -> Option<&Node>;

    fn nodes_with_label(&self, label: &Label) -> Vec<String>;

    fn neighbors(&self, id: &str, dir: Direction) -> Vec<(EdgeLabel, String)>;

    fn set_attr(&mut self, id: &str, key: &str, value: PropValue) -> bool;

    fn ingest_nodes(&mut self, nodes: Vec<Node>);

    fn ingest_edges(&mut self, edges: Vec<Edge>);
}

pub trait GraphReader {
    fn has_label(&self, label: &Label) -> bool;

    fn labels(&self) -> Vec<Label>;

    fn has_edge_label(&self, label: &EdgeLabel) -> bool;

    fn edge_labels(&self) -> Vec<EdgeLabel>;

    fn nodes_with_label(&self, label: &Label) -> Vec<NodeHandle>;

    fn all_nodes_sorted(&self) -> Vec<NodeHandle>;

    fn node(&self, h: NodeHandle) -> Option<&Node>;

    fn edge(&self, h: EdgeHandle) -> Option<&Edge>;

    fn edges_out(&self, h: NodeHandle) -> Vec<(EdgeHandle, NodeHandle)>;

    fn edges_in(&self, h: NodeHandle) -> Vec<(EdgeHandle, NodeHandle)>;

    fn index_candidates(
        &self,
        np: &NodePattern,
        where_clause: Option<&Predicate>,
        params: &BTreeMap<String, ParamBinding>,
        bound_var_prop: &dyn Fn(&str, &str) -> Option<PropValue>,
    ) -> Option<Vec<NodeHandle>>;

    fn indexed_prop_is_populated(&self, label: &Label, tag: &str) -> bool;

    fn ingest_warnings(&self) -> Vec<Warning>;
}

pub trait GraphBackend: Send + Sync {
    fn graph_view(&mut self, keyspace: &Keyspace) -> Result<&mut dyn GraphView, StoreError>;

    fn graph_reader(&self, keyspace: &Keyspace) -> Result<&dyn GraphReader, StoreError>;

    fn workspace_root(&self) -> Option<&Path>;
}

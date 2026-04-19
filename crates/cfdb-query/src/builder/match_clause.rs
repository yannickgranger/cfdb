//! `MATCH` / `OPTIONAL MATCH` / `UNWIND` clause builders.

use std::collections::BTreeMap;

use cfdb_core::{Direction, EdgeLabel, EdgePattern, Label, NodePattern, PathPattern, Pattern, PropValue};

use super::QueryBuilder;

impl QueryBuilder {
    /// `MATCH (var:Label)` — add a bare node binding.
    pub fn match_node(mut self, var: impl Into<String>, label: Label) -> Self {
        self.patterns.push(Pattern::Node(NodePattern {
            var: Some(var.into()),
            label: Some(label),
            props: BTreeMap::new(),
        }));
        self
    }

    /// `MATCH (var:Label {k: v, ...})` — node binding with inline property
    /// equalities.
    pub fn match_node_with_props(
        mut self,
        var: impl Into<String>,
        label: Label,
        props: BTreeMap<String, PropValue>,
    ) -> Self {
        self.patterns.push(Pattern::Node(NodePattern {
            var: Some(var.into()),
            label: Some(label),
            props,
        }));
        self
    }

    /// `MATCH (src)-[:EDGE_LABEL]->(dst)` — single directed hop.
    pub fn match_path(
        mut self,
        src_var: impl Into<String>,
        edge_label: EdgeLabel,
        dst_var: impl Into<String>,
    ) -> Self {
        self.patterns.push(Pattern::Path(PathPattern {
            from: NodePattern {
                var: Some(src_var.into()),
                label: None,
                props: BTreeMap::new(),
            },
            edge: EdgePattern {
                var: None,
                label: Some(edge_label),
                direction: Direction::Out,
                var_length: None,
            },
            to: NodePattern {
                var: Some(dst_var.into()),
                label: None,
                props: BTreeMap::new(),
            },
        }));
        self
    }

    /// `MATCH (src)-[:EDGE_LABEL*min..max]->(dst)` — variable-length hop.
    pub fn match_var_path(
        mut self,
        src_var: impl Into<String>,
        edge_label: EdgeLabel,
        min: u32,
        max: u32,
        dst_var: impl Into<String>,
    ) -> Self {
        self.patterns.push(Pattern::Path(PathPattern {
            from: NodePattern {
                var: Some(src_var.into()),
                label: None,
                props: BTreeMap::new(),
            },
            edge: EdgePattern {
                var: None,
                label: Some(edge_label),
                direction: Direction::Out,
                var_length: Some((min, max)),
            },
            to: NodePattern {
                var: Some(dst_var.into()),
                label: None,
                props: BTreeMap::new(),
            },
        }));
        self
    }

    /// Wrap a `Pattern` in `OPTIONAL MATCH`.
    pub fn optional(mut self, inner: Pattern) -> Self {
        self.patterns.push(Pattern::Optional(Box::new(inner)));
        self
    }

    /// `UNWIND $list_param AS var`.
    pub fn unwind(mut self, list_param: impl Into<String>, var: impl Into<String>) -> Self {
        self.patterns.push(Pattern::Unwind {
            list_param: list_param.into(),
            var: var.into(),
        });
        self
    }
}

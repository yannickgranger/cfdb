use std::collections::BTreeMap;

use cfdb_core::{
    Direction, EdgeLabel, EdgePattern, Label, NodePattern, PathPattern, Pattern, PropValue,
};

use super::QueryBuilder;

impl QueryBuilder {
    pub fn match_node(mut self, var: impl Into<String>, label: Label) -> Self {
        self.patterns.push(Pattern::Node(NodePattern {
            var: Some(var.into()),
            label: Some(label),
            props: BTreeMap::new(),
        }));
        self
    }

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

    pub fn optional(mut self, inner: Pattern) -> Self {
        self.patterns.push(Pattern::Optional(Box::new(inner)));
        self
    }

    pub fn unwind(mut self, list_param: impl Into<String>, var: impl Into<String>) -> Self {
        self.patterns.push(Pattern::Unwind {
            list_param: list_param.into(),
            var: var.into(),
        });
        self
    }
}

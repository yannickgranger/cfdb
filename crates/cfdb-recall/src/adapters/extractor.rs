use std::collections::BTreeSet;
use std::path::Path;

use cfdb_core::fact::{Node, PropValue};
use cfdb_core::schema::Label;
use cfdb_extractor::{extract_workspace, ExtractError};

use crate::PublicItem;

pub const KEPT_ITEM_KINDS: &[&str] = &[
    "fn",
    "struct",
    "enum",
    "trait",
    "type_alias",
    "const",
    "static",
    "union",
];

pub fn project_nodes(nodes: &[Node]) -> std::collections::BTreeMap<String, BTreeSet<PublicItem>> {
    let mut out: std::collections::BTreeMap<String, BTreeSet<PublicItem>> =
        std::collections::BTreeMap::new();
    nodes
        .iter()
        .filter_map(project_kept_item)
        .for_each(|(crate_name, qname)| {
            out.entry(crate_name)
                .or_default()
                .insert(PublicItem::new(qname));
        });
    out
}

fn project_kept_item(node: &Node) -> Option<(String, String)> {
    if node.label.as_str() != Label::ITEM {
        return None;
    }
    if matches!(node.props.get("is_test"), Some(PropValue::Bool(true))) {
        return None;
    }
    let PropValue::Str(kind) = node.props.get("kind")? else {
        return None;
    };
    if !KEPT_ITEM_KINDS.contains(&kind.as_str()) {
        return None;
    }
    let PropValue::Str(crate_name) = node.props.get("crate")? else {
        return None;
    };
    let PropValue::Str(qname) = node.props.get("qname")? else {
        return None;
    };
    Some((crate_name.clone(), qname.clone()))
}

pub fn extract_and_project(
    workspace_root: &Path,
) -> Result<std::collections::BTreeMap<String, BTreeSet<PublicItem>>, ExtractError> {
    let (nodes, _edges) = extract_workspace(workspace_root)?;
    Ok(project_nodes(&nodes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cfdb_core::fact::Node;
    use cfdb_core::schema::Label;
    use std::collections::BTreeMap;

    fn item_node(id: &str, crate_name: &str, qname: &str, is_test: bool) -> Node {
        item_node_with_kind(id, crate_name, qname, "struct", is_test)
    }

    fn item_node_with_kind(
        id: &str,
        crate_name: &str,
        qname: &str,
        kind: &str,
        is_test: bool,
    ) -> Node {
        let mut props = BTreeMap::new();
        props.insert("crate".into(), PropValue::Str(crate_name.into()));
        props.insert("qname".into(), PropValue::Str(qname.into()));
        props.insert("kind".into(), PropValue::Str(kind.into()));
        props.insert("is_test".into(), PropValue::Bool(is_test));
        Node {
            id: id.into(),
            label: Label::new(Label::ITEM),
            props,
        }
    }

    #[test]
    fn projects_prod_items_into_crate_keyed_sets() {
        let nodes = vec![
            item_node("item:c::foo", "c", "c::foo", false),
            item_node("item:c::bar", "c", "c::bar", false),
            item_node("item:d::baz", "d", "d::baz", false),
        ];
        let grouped = project_nodes(&nodes);
        assert_eq!(grouped.len(), 2);
        assert_eq!(
            grouped["c"],
            [PublicItem::new("c::foo"), PublicItem::new("c::bar")]
                .into_iter()
                .collect()
        );
        assert_eq!(
            grouped["d"],
            [PublicItem::new("d::baz")].into_iter().collect()
        );
    }

    #[test]
    fn drops_test_scope_items() {
        let nodes = vec![
            item_node("item:c::prod", "c", "c::prod", false),
            item_node("item:c::test_helper", "c", "c::test_helper", true),
        ];
        let grouped = project_nodes(&nodes);
        assert_eq!(grouped["c"].len(), 1);
        assert!(grouped["c"].contains(&PublicItem::new("c::prod")));
    }

    #[test]
    fn ignores_non_item_nodes() {
        let nodes = vec![
            item_node("item:c::foo", "c", "c::foo", false),
            Node {
                id: "callsite:c::foo:bar:0".into(),
                label: Label::new(Label::CALL_SITE),
                props: BTreeMap::new(),
            },
            Node {
                id: "module:c".into(),
                label: Label::new(Label::MODULE),
                props: BTreeMap::new(),
            },
        ];
        let grouped = project_nodes(&nodes);
        assert_eq!(grouped.len(), 1);
        assert_eq!(grouped["c"].len(), 1);
    }

    #[test]
    fn skips_item_nodes_missing_required_props() {
        let nodes = vec![Node {
            id: "item:c::foo".into(),
            label: Label::new(Label::ITEM),
            props: BTreeMap::new(),
        }];
        let grouped = project_nodes(&nodes);
        assert!(grouped.is_empty());
    }

    #[test]
    fn drops_methods_by_kind_filter() {
        let nodes = vec![
            item_node_with_kind("item:c::foo", "c", "c::foo", "fn", false),
            item_node_with_kind("item:c::Bar", "c", "c::Bar", "struct", false),
            item_node_with_kind("item:c::Bar::new", "c", "c::Bar::new", "method", false),
        ];
        let grouped = project_nodes(&nodes);
        assert_eq!(grouped["c"].len(), 2);
        assert!(grouped["c"].contains(&PublicItem::new("c::foo")));
        assert!(grouped["c"].contains(&PublicItem::new("c::Bar")));
        assert!(!grouped["c"].contains(&PublicItem::new("c::Bar::new")));
    }

    #[test]
    fn keeps_all_kept_item_kinds() {
        let nodes: Vec<Node> = KEPT_ITEM_KINDS
            .iter()
            .enumerate()
            .map(|(i, k)| {
                item_node_with_kind(&format!("item:c::x{i}"), "c", &format!("c::x{i}"), k, false)
            })
            .collect();
        let grouped = project_nodes(&nodes);
        assert_eq!(grouped["c"].len(), KEPT_ITEM_KINDS.len());
    }

    #[test]
    fn kept_item_kinds_correspond_to_ground_truth_list() {
        use rustdoc_types::ItemKind;
        let as_wire = |k: &ItemKind| -> &'static str {
            match k {
                ItemKind::Function => "fn",
                ItemKind::Struct => "struct",
                ItemKind::Enum => "enum",
                ItemKind::Trait => "trait",
                ItemKind::TypeAlias => "type_alias",
                ItemKind::Constant => "const",
                ItemKind::Static => "static",
                ItemKind::Union => "union",
                other => panic!(
                    "ground_truth::KEPT_ITEM_KINDS gained {other:?} with no wire mapping — update both lists"
                ),
            }
        };
        let ground_truth: std::collections::BTreeSet<&str> =
            crate::adapters::ground_truth::KEPT_ITEM_KINDS
                .iter()
                .map(as_wire)
                .collect();
        let extractor: std::collections::BTreeSet<&str> = KEPT_ITEM_KINDS.iter().copied().collect();
        assert_eq!(ground_truth, extractor);
        assert_eq!(
            crate::adapters::ground_truth::KEPT_ITEM_KINDS.len(),
            KEPT_ITEM_KINDS.len()
        );
        assert_eq!(extractor.len(), KEPT_ITEM_KINDS.len());
    }
}

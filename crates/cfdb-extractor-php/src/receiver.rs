use std::collections::{BTreeMap, BTreeSet, VecDeque};

use cfdb_core::fact::{Edge, Node, PropValue};
use cfdb_core::qname::field_node_id;
use cfdb_core::schema::EdgeLabel;

use crate::emitter::item_id;
use crate::text;

const PRIMITIVE_ARMS: &[&str] = &[
    "int", "float", "string", "bool", "array", "object", "mixed", "void", "never", "iterable",
    "callable", "true", "false", "null", "self", "static", "parent",
];

pub(crate) enum Receiver {
    This,
    ThisProperty(String),
}

pub(crate) fn classify_receiver(object: tree_sitter::Node, src: &[u8]) -> Option<Receiver> {
    if is_this_variable(object, src) {
        return Some(Receiver::This);
    }
    if object.kind() == "member_access_expression" {
        let inner = object.child_by_field_name("object")?;
        let name = object.child_by_field_name("name")?;
        if is_this_variable(inner, src) && name.kind() == "name" {
            return Some(Receiver::ThisProperty(text(name, src)?.to_string()));
        }
    }
    None
}

fn is_this_variable(node: tree_sitter::Node, src: &[u8]) -> bool {
    node.kind() == "variable_name" && text(node, src) == Some("$this")
}

fn single_class_arm(type_normalized: &str) -> Option<&str> {
    let mut candidates = type_normalized.split('|').filter(|arm| {
        !arm.is_empty() && !PRIMITIVE_ARMS.contains(&arm.to_ascii_lowercase().as_str())
    });
    let first = candidates.next()?;
    if candidates.next().is_some() {
        None
    } else {
        Some(first)
    }
}

pub(crate) fn build_extends_parents(edges: &[Edge]) -> BTreeMap<String, Vec<String>> {
    let mut map: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for edge in edges {
        if edge.label.as_str() != EdgeLabel::EXTENDS {
            continue;
        }
        let Some(src_qname) = edge.src.strip_prefix("item:") else {
            continue;
        };
        let Some(dst_qname) = edge.dst.strip_prefix("item:") else {
            continue;
        };
        map.entry(src_qname.to_string())
            .or_default()
            .push(dst_qname.to_string());
    }
    map
}

fn owner_chain(start: &str, extends_parents: &BTreeMap<String, Vec<String>>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut order = Vec::new();
    let mut queue = VecDeque::new();
    queue.push_back(start.to_string());
    seen.insert(start.to_string());
    while let Some(current) = queue.pop_front() {
        if let Some(parents) = extends_parents.get(&current) {
            for parent in parents {
                if seen.insert(parent.clone()) {
                    queue.push_back(parent.clone());
                }
            }
        }
        order.push(current);
    }
    order
}

fn find_field<'a>(
    property: &str,
    chain: &[String],
    node_ids: &BTreeMap<String, usize>,
    nodes: &'a [Node],
) -> Option<&'a Node> {
    chain.iter().find_map(|class_qname| {
        let field_id = field_node_id(class_qname, property);
        node_ids.get(&field_id).and_then(|&idx| nodes.get(idx))
    })
}

fn item_exists(qname: &str, node_ids: &BTreeMap<String, usize>) -> bool {
    node_ids.contains_key(&item_id(qname))
}

pub(crate) fn resolve_call(
    receiver: &Receiver,
    enclosing_class_qname: &str,
    method_name: &str,
    node_ids: &BTreeMap<String, usize>,
    nodes: &[Node],
    extends_parents: &BTreeMap<String, Vec<String>>,
) -> Option<String> {
    let owner = match receiver {
        Receiver::This => enclosing_class_qname.to_string(),
        Receiver::ThisProperty(property) => {
            let chain = owner_chain(enclosing_class_qname, extends_parents);
            let field = find_field(property, &chain, node_ids, nodes)?;
            let type_normalized = field
                .props
                .get("type_normalized")
                .and_then(PropValue::as_str)?;
            single_class_arm(type_normalized)?.to_string()
        }
    };
    if !item_exists(&owner, node_ids) {
        return None;
    }
    owner_chain(&owner, extends_parents)
        .into_iter()
        .map(|candidate| format!("{candidate}::{method_name}"))
        .find(|candidate_method| item_exists(candidate_method, node_ids))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cfdb_core::fact::{Edge, Node};
    use cfdb_core::schema::{EdgeLabel, Label};

    fn extends_edge(from: &str, to: &str) -> Edge {
        Edge::new(
            item_id(from),
            item_id(to),
            EdgeLabel::new(EdgeLabel::EXTENDS),
        )
    }

    fn item_node(qname: &str) -> Node {
        Node::new(item_id(qname), Label::new(Label::ITEM))
    }

    fn field_node(class_qname: &str, name: &str, type_normalized: &str) -> Node {
        Node::new(field_node_id(class_qname, name), Label::new(Label::FIELD))
            .with_prop("type_normalized", type_normalized)
    }

    fn indexed(nodes: Vec<Node>) -> (BTreeMap<String, usize>, Vec<Node>) {
        let mut node_ids = BTreeMap::new();
        for (i, node) in nodes.iter().enumerate() {
            node_ids.insert(node.id.clone(), i);
        }
        (node_ids, nodes)
    }

    #[test]
    fn this_resolves_to_a_method_declared_on_the_enclosing_class() {
        let (node_ids, nodes) = indexed(vec![item_node("App\\C"), item_node("App\\C::m")]);
        let extends_parents = BTreeMap::new();
        assert_eq!(
            resolve_call(
                &Receiver::This,
                "App\\C",
                "m",
                &node_ids,
                &nodes,
                &extends_parents
            ),
            Some("App\\C::m".to_string())
        );
    }

    #[test]
    fn this_falls_back_to_a_method_on_the_nearest_extends_ancestor() {
        let (node_ids, nodes) = indexed(vec![item_node("App\\C"), item_node("App\\Base::m")]);
        let mut extends_parents = BTreeMap::new();
        extends_parents.insert("App\\C".to_string(), vec!["App\\Base".to_string()]);
        assert_eq!(
            resolve_call(
                &Receiver::This,
                "App\\C",
                "m",
                &node_ids,
                &nodes,
                &extends_parents
            ),
            Some("App\\Base::m".to_string())
        );
    }

    #[test]
    fn this_with_no_declaration_anywhere_in_the_chain_is_unresolved() {
        let (node_ids, nodes) = indexed(vec![item_node("App\\C")]);
        let extends_parents = BTreeMap::new();
        assert_eq!(
            resolve_call(
                &Receiver::This,
                "App\\C",
                "m",
                &node_ids,
                &nodes,
                &extends_parents
            ),
            None
        );
    }

    #[test]
    fn this_property_resolves_through_a_single_class_typed_field() {
        let (node_ids, nodes) = indexed(vec![
            field_node("App\\C", "port", "App\\Port"),
            item_node("App\\Port"),
            item_node("App\\Port::m"),
        ]);
        let extends_parents = BTreeMap::new();
        assert_eq!(
            resolve_call(
                &Receiver::ThisProperty("port".to_string()),
                "App\\C",
                "m",
                &node_ids,
                &nodes,
                &extends_parents
            ),
            Some("App\\Port::m".to_string())
        );
    }

    #[test]
    fn this_property_ignores_the_null_arm_of_a_nullable_field() {
        let (node_ids, nodes) = indexed(vec![
            field_node("App\\C", "port", "App\\Port|null"),
            item_node("App\\Port"),
            item_node("App\\Port::m"),
        ]);
        let extends_parents = BTreeMap::new();
        assert_eq!(
            resolve_call(
                &Receiver::ThisProperty("port".to_string()),
                "App\\C",
                "m",
                &node_ids,
                &nodes,
                &extends_parents
            ),
            Some("App\\Port::m".to_string())
        );
    }

    #[test]
    fn this_property_with_a_two_class_union_is_unresolved() {
        let (node_ids, nodes) = indexed(vec![
            field_node("App\\C", "port", "App\\Port|App\\Other"),
            item_node("App\\Port"),
            item_node("App\\Other"),
            item_node("App\\Port::m"),
        ]);
        let extends_parents = BTreeMap::new();
        assert_eq!(
            resolve_call(
                &Receiver::ThisProperty("port".to_string()),
                "App\\C",
                "m",
                &node_ids,
                &nodes,
                &extends_parents
            ),
            None
        );
    }

    #[test]
    fn this_property_with_a_vendor_typed_field_is_unresolved() {
        let (node_ids, nodes) = indexed(vec![field_node("App\\C", "logger", "Vendor\\Logger")]);
        let extends_parents = BTreeMap::new();
        assert_eq!(
            resolve_call(
                &Receiver::ThisProperty("logger".to_string()),
                "App\\C",
                "m",
                &node_ids,
                &nodes,
                &extends_parents
            ),
            None
        );
    }

    #[test]
    fn this_property_finds_a_promoted_field_declared_on_a_parent_class() {
        let (node_ids, nodes) = indexed(vec![
            field_node("App\\Base", "port", "App\\Port"),
            item_node("App\\Port"),
            item_node("App\\Port::m"),
        ]);
        let mut extends_parents = BTreeMap::new();
        extends_parents.insert("App\\C".to_string(), vec!["App\\Base".to_string()]);
        assert_eq!(
            resolve_call(
                &Receiver::ThisProperty("port".to_string()),
                "App\\C",
                "m",
                &node_ids,
                &nodes,
                &extends_parents
            ),
            Some("App\\Port::m".to_string())
        );
    }

    #[test]
    fn build_extends_parents_preserves_source_order_for_multi_extends() {
        let edges = vec![
            extends_edge("App\\I", "App\\A"),
            extends_edge("App\\I", "App\\B"),
        ];
        let map = build_extends_parents(&edges);
        assert_eq!(
            map.get("App\\I"),
            Some(&vec!["App\\A".to_string(), "App\\B".to_string()])
        );
    }
}

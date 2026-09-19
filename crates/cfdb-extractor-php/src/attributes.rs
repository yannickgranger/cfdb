use cfdb_core::fact::{Edge, Node};
use cfdb_core::qname::attribute_node_id;
use cfdb_core::schema::{EdgeLabel, Label};

use crate::emitter::Emitter;
use crate::imports::ImportTable;
use crate::text;

fn attribute_nodes(owner: tree_sitter::Node) -> Vec<tree_sitter::Node> {
    let Some(list) = owner.child_by_field_name("attributes") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut group_cursor = list.walk();
    for group in list
        .children(&mut group_cursor)
        .filter(|c| c.kind() == "attribute_group")
    {
        let mut attr_cursor = group.walk();
        out.extend(
            group
                .children(&mut attr_cursor)
                .filter(|c| c.kind() == "attribute"),
        );
    }
    out
}

fn attribute_name(attribute: tree_sitter::Node) -> Option<tree_sitter::Node> {
    let mut cursor = attribute.walk();
    for child in attribute.children(&mut cursor) {
        if matches!(child.kind(), "name" | "qualified_name") {
            return Some(child);
        }
    }
    None
}

pub(crate) fn emit_attributes(
    owner: tree_sitter::Node,
    src: &[u8],
    current_ns: Option<&str>,
    imports: &ImportTable,
    owner_id: &str,
    file: &str,
    emitter: &mut Emitter,
) {
    for (idx, attribute) in attribute_nodes(owner).into_iter().enumerate() {
        let Some(name_node) = attribute_name(attribute) else {
            continue;
        };
        let Some(written) = text(name_node, src) else {
            continue;
        };
        let fqn = imports.resolve(written, current_ns);
        let id = attribute_node_id(owner_id, idx);

        emitter.emit_node(
            Node::new(id.as_str(), Label::new(Label::ATTRIBUTE))
                .with_prop("file", file)
                .with_prop("fqn", fqn.as_str())
                .with_prop("line", (attribute.start_position().row + 1) as i64)
                .with_prop("written", written),
        );
        emitter.emit_edge(Edge::new(
            owner_id,
            id.as_str(),
            EdgeLabel::new(EdgeLabel::HAS_ATTRIBUTE),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(source: &str) -> (tree_sitter::Tree, Vec<u8>) {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_php::LANGUAGE_PHP.into())
            .expect("tree-sitter-php grammar");
        let tree = parser.parse(source, None).expect("parse");
        (tree, source.as_bytes().to_vec())
    }

    fn find<'t>(node: tree_sitter::Node<'t>, kind: &str) -> Option<tree_sitter::Node<'t>> {
        if node.kind() == kind {
            return Some(node);
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if let Some(found) = find(child, kind) {
                return Some(found);
            }
        }
        None
    }

    #[test]
    fn a_class_with_no_attributes_yields_none() {
        let (tree, src) = parse("<?php class C {}");
        let class = find(tree.root_node(), "class_declaration").expect("class_declaration");
        assert!(attribute_nodes(class).is_empty());
        let _ = src;
    }

    #[test]
    fn a_grouped_attribute_yields_two_in_source_order() {
        let (tree, src) = parse("<?php #[A, B] class C {}");
        let class = find(tree.root_node(), "class_declaration").expect("class_declaration");
        let attrs = attribute_nodes(class);
        assert_eq!(attrs.len(), 2);
        let names: Vec<&str> = attrs
            .iter()
            .map(|a| text(attribute_name(*a).unwrap(), &src).unwrap())
            .collect();
        assert_eq!(names, vec!["A", "B"]);
    }
}

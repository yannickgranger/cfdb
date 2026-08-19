use std::collections::{BTreeMap, BTreeSet};

use cfdb_core::fact::{build_item_props_common, Edge, Node, PropValue};
use cfdb_core::schema::{EdgeLabel, Label};
use tree_sitter::Node as TsNode;

use super::PRODUCER_NAME;

#[allow(clippy::too_many_arguments)]
pub(crate) fn emit_class_methods(
    class_decl: TsNode<'_>,
    source: &[u8],
    class_name: &str,
    crate_name: &str,
    crate_id: &str,
    module_qpath: &str,
    module_id: &str,
    rel_path: &str,
    nodes: &mut Vec<Node>,
    edges: &mut Vec<Edge>,
) {
    let mut bcursor = class_decl.walk();
    for body in class_decl.children(&mut bcursor) {
        if body.kind() != "class_body" {
            continue;
        }
        let mut seen: BTreeSet<String> = BTreeSet::new();
        let mut mcursor = body.walk();
        for member in body.children(&mut mcursor) {
            let Some(method) = method_member_name(member, source) else {
                continue;
            };
            let qname = format!("{crate_name}::{module_qpath}::{class_name}::{method}");
            if seen.contains(&qname) {
                continue;
            }
            emit_method_item(
                &qname,
                &method,
                member,
                crate_name,
                crate_id,
                module_qpath,
                module_id,
                rel_path,
                nodes,
                edges,
            );
            crate::call_walker::walk_call_sites(member, source, &qname, rel_path, nodes, edges);
            seen.insert(qname);
        }
    }
}

fn method_member_name(member: TsNode<'_>, source: &[u8]) -> Option<String> {
    match member.kind() {
        "method_definition" => field_text(member, "name", source),
        "public_field_definition" => {
            let value = member.child_by_field_name("value")?;
            if value.kind() == "arrow_function" {
                field_text(member, "name", source)
            } else {
                None
            }
        }
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_method_item(
    qname: &str,
    name: &str,
    member: TsNode<'_>,
    crate_name: &str,
    crate_id: &str,
    module_qpath: &str,
    module_id: &str,
    rel_path: &str,
    nodes: &mut Vec<Node>,
    edges: &mut Vec<Edge>,
) {
    let id = format!("item:{qname}");
    let line = (member.start_position().row + 1) as i64;

    let mut props = build_item_props_common(qname, name, "fn", crate_name);
    props.insert(
        "module_qpath".into(),
        PropValue::Str(module_qpath.to_string()),
    );
    props.insert("file".into(), PropValue::Str(rel_path.to_string()));
    props.insert("line".into(), PropValue::Int(line));
    props.insert("is_test".into(), PropValue::Bool(false));
    props.insert(
        "visibility".into(),
        PropValue::Str(method_visibility(member)),
    );
    props.insert("language".into(), PropValue::Str(PRODUCER_NAME.into()));

    nodes.push(Node {
        id: id.clone(),
        label: Label::new(Label::ITEM),
        props,
    });
    edges.push(Edge {
        src: id.clone(),
        dst: crate_id.to_string(),
        label: EdgeLabel::new(EdgeLabel::IN_CRATE),
        props: BTreeMap::new(),
    });
    edges.push(Edge {
        src: id,
        dst: module_id.to_string(),
        label: EdgeLabel::new(EdgeLabel::IN_MODULE),
        props: BTreeMap::new(),
    });
}

fn method_visibility(member: TsNode<'_>) -> String {
    let mut cursor = member.walk();
    for child in member.children(&mut cursor) {
        if child.kind() == "accessibility_modifier" {
            return child.child(0).map_or("public", |kw| kw.kind()).to_string();
        }
    }
    "public".to_string()
}

fn field_text(node: TsNode<'_>, field: &str, source: &[u8]) -> Option<String> {
    let child = node.child_by_field_name(field)?;
    std::str::from_utf8(&source[child.byte_range()])
        .ok()
        .map(|s| s.to_string())
}

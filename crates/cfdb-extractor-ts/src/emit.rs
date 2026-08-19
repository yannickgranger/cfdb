use std::collections::BTreeMap;

use cfdb_core::fact::{build_item_props_common, Edge, Node, PropValue, Props};
use cfdb_core::schema::{EdgeLabel, Label};
use tree_sitter::Node as TsNode;

use super::PRODUCER_NAME;

#[allow(clippy::too_many_arguments)]
pub(crate) fn walk_program(
    root: TsNode<'_>,
    source: &[u8],
    crate_name: &str,
    crate_id: &str,
    module_qpath: &str,
    module_id: &str,
    rel_path: &str,
    nodes: &mut Vec<Node>,
    edges: &mut Vec<Edge>,
    pending_implements: &mut Vec<(String, String)>,
) {
    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        let (decl, exported) = unwrap_export(child);
        if let Some(decl_node) = decl {
            emit_top_level_declaration(
                decl_node,
                exported,
                source,
                crate_name,
                crate_id,
                module_qpath,
                module_id,
                rel_path,
                nodes,
                edges,
                pending_implements,
            );
        }
    }
}

fn unwrap_export(node: TsNode<'_>) -> (Option<TsNode<'_>>, bool) {
    if node.kind() == "export_statement" {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "interface_declaration"
                | "type_alias_declaration"
                | "class_declaration"
                | "function_declaration"
                | "lexical_declaration"
                | "variable_declaration"
                | "abstract_class_declaration" => return (Some(child), true),
                _ => {}
            }
        }
        (None, true)
    } else {
        (Some(node), false)
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_top_level_declaration(
    decl: TsNode<'_>,
    exported: bool,
    source: &[u8],
    crate_name: &str,
    crate_id: &str,
    module_qpath: &str,
    module_id: &str,
    rel_path: &str,
    nodes: &mut Vec<Node>,
    edges: &mut Vec<Edge>,
    pending_implements: &mut Vec<(String, String)>,
) {
    let (name, kind) = match decl.kind() {
        "interface_declaration" => (named_child_text(decl, "name", source), "trait"),
        "type_alias_declaration" => (named_child_text(decl, "name", source), "type"),
        "class_declaration" | "abstract_class_declaration" => {
            (named_child_text(decl, "name", source), "struct")
        }
        "function_declaration" => (named_child_text(decl, "name", source), "fn"),
        "lexical_declaration" | "variable_declaration" => {
            emit_variable_declarators(
                decl,
                exported,
                source,
                crate_name,
                crate_id,
                module_qpath,
                module_id,
                rel_path,
                nodes,
                edges,
            );
            return;
        }
        _ => return,
    };
    let Some(name) = name else { return };
    let id = emit_item_node(
        &name,
        kind,
        decl,
        exported,
        crate_name,
        crate_id,
        module_qpath,
        module_id,
        rel_path,
        nodes,
        edges,
    );

    if let "class_declaration" | "abstract_class_declaration" = decl.kind() {
        buffer_implements_targets(decl, source, &id, pending_implements);
        crate::methods::emit_class_methods(
            decl,
            source,
            &name,
            crate_name,
            crate_id,
            module_qpath,
            module_id,
            rel_path,
            nodes,
            edges,
        );
    }

    if decl.kind() == "function_declaration" {
        let caller_qname = id.strip_prefix("item:").unwrap_or(&id);
        crate::call_walker::walk_call_sites(decl, source, caller_qname, rel_path, nodes, edges);
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_variable_declarators(
    decl: TsNode<'_>,
    exported: bool,
    source: &[u8],
    crate_name: &str,
    crate_id: &str,
    module_qpath: &str,
    module_id: &str,
    rel_path: &str,
    nodes: &mut Vec<Node>,
    edges: &mut Vec<Edge>,
) {
    let mut cursor = decl.walk();
    for child in decl.children(&mut cursor) {
        if child.kind() != "variable_declarator" {
            continue;
        }
        let Some(name) = named_child_text(child, "name", source) else {
            continue;
        };
        let _ = emit_item_node(
            &name,
            "const",
            child,
            exported,
            crate_name,
            crate_id,
            module_qpath,
            module_id,
            rel_path,
            nodes,
            edges,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_item_node(
    name: &str,
    kind: &str,
    decl: TsNode<'_>,
    exported: bool,
    crate_name: &str,
    crate_id: &str,
    module_qpath: &str,
    module_id: &str,
    rel_path: &str,
    nodes: &mut Vec<Node>,
    edges: &mut Vec<Edge>,
) -> String {
    let qname = format!("{crate_name}::{module_qpath}::{name}");
    let id = format!("item:{qname}");
    let line = (decl.start_position().row + 1) as i64;
    let visibility = if exported { "public" } else { "private" };

    let mut props = build_item_props_common(&qname, name, kind, crate_name);
    props.insert(
        "module_qpath".into(),
        PropValue::Str(module_qpath.to_string()),
    );
    props.insert("file".into(), PropValue::Str(rel_path.to_string()));
    props.insert("line".into(), PropValue::Int(line));
    props.insert("is_test".into(), PropValue::Bool(false));
    props.insert("visibility".into(), PropValue::Str(visibility.into()));
    props.insert("language".into(), PropValue::Str(PRODUCER_NAME.into()));
    if let "class_declaration" | "abstract_class_declaration" | "interface_declaration" =
        decl.kind()
    {
        props.insert(
            "ts_construct".into(),
            PropValue::Str(decl.kind().to_string()),
        );
    }

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
        src: id.clone(),
        dst: module_id.to_string(),
        label: EdgeLabel::new(EdgeLabel::IN_MODULE),
        props: BTreeMap::new(),
    });
    id
}

fn named_child_text(node: TsNode<'_>, field: &str, source: &[u8]) -> Option<String> {
    let child = node.child_by_field_name(field)?;
    let bytes = &source[child.byte_range()];
    std::str::from_utf8(bytes).ok().map(|s| s.to_string())
}

fn buffer_implements_targets(
    class_decl: TsNode<'_>,
    source: &[u8],
    class_id: &str,
    pending: &mut Vec<(String, String)>,
) {
    let mut hcursor = class_decl.walk();
    for heritage in class_decl.children(&mut hcursor) {
        if heritage.kind() != "class_heritage" {
            continue;
        }
        let mut ccursor = heritage.walk();
        for clause in heritage.children(&mut ccursor) {
            if clause.kind() == "implements_clause" {
                buffer_clause_targets(clause, source, class_id, pending);
            }
        }
    }
}

fn buffer_clause_targets(
    clause: TsNode<'_>,
    source: &[u8],
    class_id: &str,
    pending: &mut Vec<(String, String)>,
) {
    let mut rcursor = clause.walk();
    for type_ref in clause.children(&mut rcursor) {
        if !type_ref.is_named() {
            continue;
        }
        if let Ok(text) = std::str::from_utf8(&source[type_ref.byte_range()]) {
            pending.push((class_id.to_string(), text.to_string()));
        }
    }
}

pub(crate) fn resolve_implements(
    pending: Vec<(String, String)>,
    nodes: &[Node],
    edges: &mut Vec<Edge>,
) {
    let mut by_name: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for n in nodes {
        if n.label.as_str() != Label::ITEM {
            continue;
        }
        if !matches!(
            n.props.get("kind").and_then(PropValue::as_str),
            Some("trait" | "struct")
        ) {
            continue;
        }
        if let Some(name) = n.props.get("name").and_then(PropValue::as_str) {
            by_name.entry(name).or_default().push(n.id.as_str());
        }
    }

    for (class_id, ref_text) in pending {
        let Some(ids) = by_name.get(ref_text.as_str()) else {
            continue;
        };
        if ids.len() != 1 {
            continue;
        }
        let mut props = Props::new();
        props.insert(
            "resolver".into(),
            PropValue::Str("tree-sitter-typescript".into()),
        );
        edges.push(Edge {
            src: class_id,
            dst: ids[0].to_string(),
            label: EdgeLabel::new(EdgeLabel::IMPLEMENTS),
            props,
        });
    }
}

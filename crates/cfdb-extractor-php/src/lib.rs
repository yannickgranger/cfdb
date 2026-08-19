use std::path::{Path, PathBuf};

use cfdb_core::fact::{Edge, Node, PropValue};
use cfdb_core::schema::{EdgeLabel, Label};
use cfdb_lang::{LanguageError, LanguageProducer};

mod call_walker;
mod emitter;
mod implements;
use emitter::{item_id, module_id, Emitter};

const PRODUCER_NAME: &str = "php";

const CRATE_ID: &str = "crate:php-workspace";

pub struct PhpProducer;

impl LanguageProducer for PhpProducer {
    fn name(&self) -> &'static str {
        PRODUCER_NAME
    }

    fn detect(&self, workspace_root: &Path) -> bool {
        workspace_root.join("composer.json").is_file()
    }

    fn produce(&self, workspace_root: &Path) -> Result<(Vec<Node>, Vec<Edge>), LanguageError> {
        produce_facts(workspace_root)
    }
}

fn produce_facts(workspace_root: &Path) -> Result<(Vec<Node>, Vec<Edge>), LanguageError> {
    let workspace_root = cfdb_lang::canonical_workspace_root(workspace_root)?;
    let workspace_root = workspace_root.as_path();

    let mut emitter = Emitter::new();

    emitter.emit_node(
        Node::new(CRATE_ID, Label::new(Label::CRATE))
            .with_prop("name", "php-workspace")
            .with_prop("is_workspace_member", true),
    );

    let php_files = collect_php_files(workspace_root)?;
    for path in php_files {
        let file = cfdb_lang::workspace_relative(&path, workspace_root, PRODUCER_NAME)?;
        walk_file(&path, &file, &mut emitter)?;
    }

    emitter.resolve_pending_implements();
    emitter.resolve_pending_call_sites();

    let (mut nodes, mut edges) = emitter.finish();
    nodes.sort_by(|a, b| a.sort_key().cmp(&b.sort_key()));
    edges.sort_by(|a, b| a.sort_key().cmp(&b.sort_key()));
    Ok((nodes, edges))
}

fn collect_php_files(workspace_root: &Path) -> Result<Vec<PathBuf>, LanguageError> {
    let mut out = Vec::new();
    walk_dir(workspace_root, &mut out)?;
    out.sort();
    Ok(out)
}

fn walk_dir(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), LanguageError> {
    let read = std::fs::read_dir(dir).map_err(LanguageError::Io)?;
    for entry in read {
        let entry = entry.map_err(LanguageError::Io)?;
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n == "vendor") {
                continue;
            }
            walk_dir(&path, out)?;
        } else if path.extension().is_some_and(|e| e == "php") {
            out.push(path);
        }
    }
    Ok(())
}

fn walk_file(path: &Path, file: &str, emitter: &mut Emitter) -> Result<(), LanguageError> {
    let source = std::fs::read_to_string(path).map_err(LanguageError::Io)?;

    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_php::LANGUAGE_PHP.into())
        .map_err(|e| LanguageError::Parse {
            producer: PRODUCER_NAME,
            message: format!("set_language: {e}"),
        })?;
    let tree = parser.parse(&source, None).ok_or(LanguageError::Parse {
        producer: PRODUCER_NAME,
        message: format!("tree-sitter-php returned None for {}", path.display()),
    })?;

    let root = tree.root_node();
    walk_top_level(root, source.as_bytes(), file, emitter);
    Ok(())
}

fn walk_top_level(program: tree_sitter::Node, src: &[u8], file: &str, emitter: &mut Emitter) {
    let mut current_ns: Option<String> = None;
    let mut cursor = program.walk();
    for child in program.children(&mut cursor) {
        match child.kind() {
            "namespace_definition" => {
                let ns_name = extract_namespace_name(child, src);
                if let Some(name) = &ns_name {
                    emit_module(emitter, name);
                }
                current_ns = ns_name;
            }
            "class_declaration" | "interface_declaration" | "trait_declaration" => {
                emit_class_like(child, src, current_ns.as_deref(), file, emitter);
            }
            "function_definition" => {
                emit_function(child, src, current_ns.as_deref(), file, emitter);
            }
            _ => {}
        }
    }
}

fn extract_namespace_name(ns_node: tree_sitter::Node, src: &[u8]) -> Option<String> {
    let mut cursor = ns_node.walk();
    for child in ns_node.children(&mut cursor) {
        if child.kind() == "namespace_name" {
            return text(child, src).map(|s| s.to_string());
        }
    }
    None
}

fn emit_module(emitter: &mut Emitter, namespace: &str) {
    let id = module_id(namespace);
    if emitter.has_node(&id) {
        return;
    }
    emitter.emit_node(
        Node::new(&id, Label::new(Label::MODULE))
            .with_prop("name", namespace)
            .with_prop("path", namespace.replace('\\', "::")),
    );
}

fn emit_class_like(
    node: tree_sitter::Node,
    src: &[u8],
    current_ns: Option<&str>,
    file: &str,
    emitter: &mut Emitter,
) {
    let Some(name) = find_named_child(node, "name", src) else {
        return;
    };
    let qname = qualify(current_ns, &name);
    let id = item_id(&qname);

    let line = (node.start_position().row + 1) as i64;
    emitter.emit_node(
        Node::new(&id, Label::new(Label::ITEM))
            .with_prop("kind", "trait")
            .with_prop("name", name.as_str())
            .with_prop("qname", qname.as_str())
            .with_prop("line", line)
            .with_prop("php_construct", node.kind()),
    );
    emitter.emit_edge(Edge::new(
        &id,
        CRATE_ID,
        EdgeLabel::new(EdgeLabel::IN_CRATE),
    ));
    if let Some(ns) = current_ns {
        emitter.emit_edge(Edge::new(
            &id,
            module_id(ns),
            EdgeLabel::new(EdgeLabel::IN_MODULE),
        ));
    }

    let mut clause_cursor = node.walk();
    for child in node.children(&mut clause_cursor) {
        if child.kind() == "class_interface_clause" {
            implements::buffer_implements_targets(child, src, current_ns, &id, emitter);
        }
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "declaration_list" {
            walk_declaration_list(child, src, current_ns, &qname, file, emitter);
        }
    }
}

fn walk_declaration_list(
    list: tree_sitter::Node,
    src: &[u8],
    current_ns: Option<&str>,
    parent_qname: &str,
    file: &str,
    emitter: &mut Emitter,
) {
    let mut cursor = list.walk();
    for child in list.children(&mut cursor) {
        if child.kind() == "method_declaration" {
            emit_method(child, src, current_ns, parent_qname, file, emitter);
        }
    }
}

fn emit_method(
    node: tree_sitter::Node,
    src: &[u8],
    current_ns: Option<&str>,
    parent_qname: &str,
    file: &str,
    emitter: &mut Emitter,
) {
    let Some(name) = find_named_child(node, "name", src) else {
        return;
    };
    let qname = format!("{parent_qname}::{name}");
    let id = item_id(&qname);
    let line = (node.start_position().row + 1) as i64;
    emitter.emit_node(
        Node::new(&id, Label::new(Label::ITEM))
            .with_prop("kind", "fn")
            .with_prop("name", name.as_str())
            .with_prop("qname", qname.as_str())
            .with_prop("line", line)
            .with_prop("php_construct", "method_declaration"),
    );
    emitter.emit_edge(Edge::new(
        &id,
        CRATE_ID,
        EdgeLabel::new(EdgeLabel::IN_CRATE),
    ));
    if let Some(ns) = current_ns {
        emitter.emit_edge(Edge::new(
            &id,
            module_id(ns),
            EdgeLabel::new(EdgeLabel::IN_MODULE),
        ));
    }

    call_walker::walk_call_sites(
        node,
        src,
        &qname,
        Some(parent_qname),
        current_ns,
        file,
        emitter,
    );
}

fn emit_function(
    node: tree_sitter::Node,
    src: &[u8],
    current_ns: Option<&str>,
    file: &str,
    emitter: &mut Emitter,
) {
    let Some(name) = find_named_child(node, "name", src) else {
        return;
    };
    let qname = qualify(current_ns, &name);
    let id = item_id(&qname);
    let line = (node.start_position().row + 1) as i64;
    emitter.emit_node(
        Node::new(&id, Label::new(Label::ITEM))
            .with_prop("kind", "fn")
            .with_prop("name", name.as_str())
            .with_prop("qname", qname.as_str())
            .with_prop("line", line)
            .with_prop("php_construct", "function_definition"),
    );
    emitter.emit_edge(Edge::new(
        &id,
        CRATE_ID,
        EdgeLabel::new(EdgeLabel::IN_CRATE),
    ));
    if let Some(ns) = current_ns {
        emitter.emit_edge(Edge::new(
            &id,
            module_id(ns),
            EdgeLabel::new(EdgeLabel::IN_MODULE),
        ));
    }

    call_walker::walk_call_sites(node, src, &qname, None, current_ns, file, emitter);
}

fn find_named_child(node: tree_sitter::Node, kind: &str, src: &[u8]) -> Option<String> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == kind {
            return text(child, src).map(|s| s.to_string());
        }
    }
    None
}

pub(crate) fn text<'s>(node: tree_sitter::Node, src: &'s [u8]) -> Option<&'s str> {
    std::str::from_utf8(&src[node.byte_range()]).ok()
}

pub(crate) fn qualify(ns: Option<&str>, name: &str) -> String {
    match ns {
        Some(ns) if !ns.is_empty() => format!("{ns}\\{name}"),
        _ => name.to_string(),
    }
}

#[allow(dead_code)]
fn _ensure_prop_value_in_use(v: PropValue) -> PropValue {
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn php_producer_is_object_safe() {
        fn _accept(_: &dyn LanguageProducer) {}
        _accept(&PhpProducer);
    }
}

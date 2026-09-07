use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use cfdb_core::fact::{Edge, Node, PropValue};
use cfdb_core::qname::{file_node_id, import_node_id};
use cfdb_core::schema::{EdgeLabel, Label};
use cfdb_lang::{LanguageError, LanguageProducer};

mod call_walker;
mod emitter;
mod implements;
mod imports;
mod test_scope;
use emitter::{item_id, module_id, Emitter};
use test_scope::ComposerScope;

pub(crate) const PRODUCER_NAME: &str = "php";

const CRATE_NAME: &str = "php-workspace";

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

    let scope = ComposerScope::from_composer(workspace_root)?;
    let php_files = collect_php_files(workspace_root, &scope)?;
    let mut emitter = Emitter::new(scope);

    emitter.emit_node(
        Node::new(CRATE_ID, Label::new(Label::CRATE))
            .with_prop("name", CRATE_NAME)
            .with_prop("is_workspace_member", true),
    );

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

fn collect_php_files(
    workspace_root: &Path,
    scope: &ComposerScope,
) -> Result<Vec<PathBuf>, LanguageError> {
    let mut out = Vec::new();
    for root in scope.declared_roots() {
        let path = workspace_root.join(root);
        if path.is_dir() {
            walk_dir(&path, &mut out)?;
        } else if path.is_file() && path.extension().is_some_and(|e| e == "php") {
            out.push(path);
        }
    }
    out.sort();
    out.dedup();
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
    let imports = imports::collect(program, src);
    emit_file_and_imports(file, &imports.declarations, emitter);
    let imports = imports.table;
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
            "class_declaration"
            | "interface_declaration"
            | "trait_declaration"
            | "enum_declaration" => {
                emit_class_like(child, src, current_ns.as_deref(), &imports, file, emitter);
            }
            "function_definition" => {
                emit_function(child, src, current_ns.as_deref(), &imports, file, emitter);
            }
            _ => {}
        }
    }
}

fn emit_file_and_imports(file: &str, declarations: &[imports::Declaration], emitter: &mut Emitter) {
    let file_id = file_node_id(CRATE_NAME, file);
    emitter.emit_node(
        Node::new(&file_id, Label::new(Label::FILE))
            .with_prop("path", file)
            .with_prop("crate", CRATE_NAME),
    );

    let mut ordinals: BTreeMap<&str, usize> = BTreeMap::new();
    for declaration in declarations {
        let ordinal = ordinals.entry(declaration.fqn.as_str()).or_insert(0);
        let id = import_node_id(file, &declaration.fqn, *ordinal);
        *ordinal += 1;

        let mut node = Node::new(&id, Label::new(Label::IMPORT))
            .with_prop("fqn", declaration.fqn.as_str())
            .with_prop("file", file)
            .with_prop("line", declaration.line);
        if let Some(alias) = &declaration.alias {
            node = node.with_prop("alias", alias.as_str());
        }
        emitter.emit_node(node);
        emitter.emit_edge(Edge::new(
            &file_id,
            &id,
            EdgeLabel::new(EdgeLabel::HAS_IMPORT),
        ));
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
    let node = Node::new(&id, Label::new(Label::MODULE))
        .with_prop("name", namespace)
        .with_prop("path", namespace.replace('\\', "::"));
    if emitter.node(&id) == Some(&node) {
        return;
    }
    emitter.emit_node(node);
}

fn emit_class_like(
    node: tree_sitter::Node,
    src: &[u8],
    current_ns: Option<&str>,
    imports: &imports::ImportTable,
    file: &str,
    emitter: &mut Emitter,
) {
    let Some(name) = find_named_child(node, "name", src) else {
        return;
    };
    let qname = qualify(current_ns, &name);
    let id = item_id(&qname);

    let line = (node.start_position().row + 1) as i64;
    let kind = if node.kind() == "enum_declaration" {
        "enum"
    } else {
        "trait"
    };
    emitter.emit_node(
        Node::new(&id, Label::new(Label::ITEM))
            .with_prop("kind", kind)
            .with_prop("name", name.as_str())
            .with_prop("qname", qname.as_str())
            .with_prop("line", line)
            .with_prop("php_construct", node.kind())
            .with_prop("file", file),
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
            implements::buffer_implements_targets(child, src, current_ns, imports, &id, emitter);
        }
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if matches!(child.kind(), "declaration_list" | "enum_declaration_list") {
            walk_declaration_list(child, src, current_ns, imports, &qname, file, emitter);
        }
    }
}

fn walk_declaration_list(
    list: tree_sitter::Node,
    src: &[u8],
    current_ns: Option<&str>,
    imports: &imports::ImportTable,
    parent_qname: &str,
    file: &str,
    emitter: &mut Emitter,
) {
    let mut cursor = list.walk();
    for child in list.children(&mut cursor) {
        if child.kind() == "method_declaration" {
            emit_method(child, src, current_ns, imports, parent_qname, file, emitter);
        }
    }
}

fn emit_method(
    node: tree_sitter::Node,
    src: &[u8],
    current_ns: Option<&str>,
    imports: &imports::ImportTable,
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
            .with_prop("php_construct", "method_declaration")
            .with_prop("file", file),
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
        &call_walker::CallScope {
            caller_qname: &qname,
            enclosing_class_qname: Some(parent_qname),
            current_ns,
            imports,
            file,
        },
        emitter,
    );
}

fn emit_function(
    node: tree_sitter::Node,
    src: &[u8],
    current_ns: Option<&str>,
    imports: &imports::ImportTable,
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
            .with_prop("php_construct", "function_definition")
            .with_prop("file", file),
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
        &call_walker::CallScope {
            caller_qname: &qname,
            enclosing_class_qname: None,
            current_ns,
            imports,
            file,
        },
        emitter,
    );
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

    fn module_nodes(emitter: Emitter) -> Vec<Node> {
        let (nodes, _) = emitter.finish();
        nodes
            .into_iter()
            .filter(|n| n.label.as_str() == Label::MODULE)
            .collect()
    }

    #[test]
    fn a_namespace_seen_twice_yields_one_module() {
        let mut emitter = Emitter::new(ComposerScope::default());
        emit_module(&mut emitter, "App");
        emit_module(&mut emitter, "App");
        let modules = module_nodes(emitter);
        assert_eq!(
            modules.len(),
            1,
            "an unchanged module must not be emitted twice, got {modules:?}"
        );
    }

    #[test]
    fn a_module_that_differs_from_the_stored_one_is_emitted_too() {
        let mut emitter = Emitter::new(ComposerScope::default());
        emitter.emit_node(
            Node::new(module_id("App"), Label::new(Label::MODULE))
                .with_prop("name", "App")
                .with_prop("path", "App")
                .with_prop("file", "src/legacy/Thing.php"),
        );
        emit_module(&mut emitter, "App");
        let modules = module_nodes(emitter);
        assert_eq!(
            modules.len(),
            2,
            "a module node that differs from the stored one must reach ingest, not be dropped by the guard: {modules:?}"
        );
    }
}

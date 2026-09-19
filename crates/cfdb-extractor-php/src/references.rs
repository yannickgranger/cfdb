use crate::emitter::Emitter;
use crate::imports::ImportTable;
use crate::text;

pub(crate) const HOW_NEW: &str = "new";
pub(crate) const HOW_INSTANCEOF: &str = "instanceof";
pub(crate) const HOW_CATCH: &str = "catch";
pub(crate) const HOW_SCOPE: &str = "scope";
pub(crate) const HOW_CLOSURE_TYPE: &str = "closure_type";
pub(crate) const HOW_TRAIT_USE: &str = "trait_use";

pub(crate) struct NameScope<'a> {
    pub current_ns: Option<&'a str>,
    pub imports: &'a ImportTable,
    pub source_qname: &'a str,
}

fn named_children(node: tree_sitter::Node) -> Vec<tree_sitter::Node> {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .filter(tree_sitter::Node::is_named)
        .collect()
}

fn resolved_name(node: tree_sitter::Node, src: &[u8], scope: &NameScope<'_>) -> Option<String> {
    if !matches!(node.kind(), "name" | "qualified_name") {
        return None;
    }
    let raw = text(node, src)?;
    if matches!(
        raw.to_ascii_lowercase().as_str(),
        "self" | "static" | "parent"
    ) {
        return None;
    }
    Some(scope.imports.resolve(raw, scope.current_ns))
}

fn buffer(
    node: tree_sitter::Node,
    src: &[u8],
    scope: &NameScope<'_>,
    how: &'static str,
    emitter: &mut Emitter,
) {
    if let Some(target) = resolved_name(node, src, scope) {
        let line = (node.start_position().row + 1) as i64;
        emitter.buffer_reference(scope.source_qname, &target, how, line);
    }
}

fn buffer_named_types(
    node: tree_sitter::Node,
    src: &[u8],
    scope: &NameScope<'_>,
    how: &'static str,
    emitter: &mut Emitter,
) {
    if matches!(node.kind(), "anonymous_function" | "arrow_function") {
        return;
    }
    if node.kind() == "named_type" {
        if let Some(name) = named_children(node).into_iter().next() {
            buffer(name, src, scope, how, emitter);
        }
        return;
    }
    for child in named_children(node) {
        buffer_named_types(child, src, scope, how, emitter);
    }
}

pub(crate) fn classify(
    node: tree_sitter::Node,
    src: &[u8],
    scope: &NameScope<'_>,
    emitter: &mut Emitter,
) {
    match node.kind() {
        "object_creation_expression" => {
            if let Some(class) = named_children(node).into_iter().next() {
                buffer(class, src, scope, HOW_NEW, emitter);
            }
        }
        "binary_expression" => {
            let is_instanceof = node
                .child_by_field_name("operator")
                .and_then(|op| text(op, src))
                .is_some_and(|op| op.eq_ignore_ascii_case("instanceof"));
            if is_instanceof {
                if let Some(right) = node.child_by_field_name("right") {
                    buffer(right, src, scope, HOW_INSTANCEOF, emitter);
                }
            }
        }
        "catch_clause" => {
            if let Some(types) = node.child_by_field_name("type") {
                buffer_named_types(types, src, scope, HOW_CATCH, emitter);
            }
        }
        "class_constant_access_expression" => {
            if let Some(class) = named_children(node).into_iter().next() {
                buffer(class, src, scope, HOW_SCOPE, emitter);
            }
        }
        "scoped_property_access_expression" | "scoped_call_expression" => {
            if let Some(class) = node.child_by_field_name("scope") {
                buffer(class, src, scope, HOW_SCOPE, emitter);
            }
        }
        "anonymous_function" | "arrow_function" => {
            for field in ["parameters", "return_type"] {
                if let Some(declared) = node.child_by_field_name(field) {
                    for child in named_children(declared) {
                        buffer_named_types(child, src, scope, HOW_CLOSURE_TYPE, emitter);
                    }
                    if declared.kind() == "named_type" {
                        buffer_named_types(declared, src, scope, HOW_CLOSURE_TYPE, emitter);
                    }
                }
            }
        }
        _ => {}
    }
}

pub(crate) fn walk_expression(
    node: tree_sitter::Node,
    src: &[u8],
    scope: &NameScope<'_>,
    emitter: &mut Emitter,
) {
    classify(node, src, scope, emitter);
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_expression(child, src, scope, emitter);
    }
}

pub(crate) fn buffer_trait_use(
    use_declaration: tree_sitter::Node,
    src: &[u8],
    scope: &NameScope<'_>,
    emitter: &mut Emitter,
) {
    for child in named_children(use_declaration) {
        buffer(child, src, scope, HOW_TRAIT_USE, emitter);
    }
}

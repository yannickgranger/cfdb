use cfdb_core::fact::{Edge, Node};
use cfdb_core::qname::{field_node_id, param_node_id};
use cfdb_core::schema::{EdgeLabel, Label};

use crate::attributes;
use crate::emitter::Emitter;
use crate::imports::ImportTable;
use crate::text;

pub(crate) struct TypeCtx<'a> {
    pub current_ns: Option<&'a str>,
    pub imports: &'a ImportTable,
    pub enclosing_class_qname: Option<&'a str>,
    pub enclosing_class_parent: Option<&'a str>,
}

pub(crate) struct ResolvedType {
    pub normalized: String,
    named_targets: Vec<String>,
}

fn named_children(node: tree_sitter::Node) -> Vec<tree_sitter::Node> {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .filter(tree_sitter::Node::is_named)
        .collect()
}

fn variable_name_text<'s>(node: tree_sitter::Node, src: &'s [u8]) -> Option<&'s str> {
    match node.kind() {
        "variable_name" => named_children(node)
            .into_iter()
            .next()
            .and_then(|n| text(n, src)),
        "by_ref" => named_children(node)
            .into_iter()
            .next()
            .and_then(|n| variable_name_text(n, src)),
        _ => None,
    }
}

fn resolve_named_type(node: tree_sitter::Node, src: &[u8], ctx: &TypeCtx) -> String {
    let raw = named_children(node)
        .into_iter()
        .next()
        .and_then(|n| text(n, src))
        .unwrap_or_default();
    match raw.to_ascii_lowercase().as_str() {
        "self" | "static" => ctx
            .enclosing_class_qname
            .map(str::to_string)
            .unwrap_or_else(|| raw.to_string()),
        "parent" => ctx
            .enclosing_class_parent
            .map(str::to_string)
            .unwrap_or_else(|| "parent".to_string()),
        _ => ctx.imports.resolve(raw, ctx.current_ns),
    }
}

fn resolve_type_node(
    node: tree_sitter::Node,
    src: &[u8],
    ctx: &TypeCtx,
) -> (Vec<String>, Vec<String>) {
    match node.kind() {
        "primitive_type" | "bottom_type" => {
            let arm = text(node, src).unwrap_or_default().to_ascii_lowercase();
            (vec![arm], vec![])
        }
        "named_type" => {
            let fqn = resolve_named_type(node, src, ctx);
            (vec![fqn.clone()], vec![fqn])
        }
        "optional_type" => {
            let Some(inner) = named_children(node).into_iter().next() else {
                return (vec![], vec![]);
            };
            let (mut arms, targets) = resolve_type_node(inner, src, ctx);
            arms.push("null".to_string());
            (arms, targets)
        }
        "union_type" => {
            let mut arms = Vec::new();
            let mut targets = Vec::new();
            for child in named_children(node) {
                let (a, t) = resolve_type_node(child, src, ctx);
                arms.extend(a);
                targets.extend(t);
            }
            (arms, targets)
        }
        "intersection_type" => {
            let (joined, targets) = resolve_intersection(node, src, ctx);
            (vec![joined], targets)
        }
        "disjunctive_normal_form_type" => {
            let mut arms = Vec::new();
            let mut targets = Vec::new();
            for child in named_children(node) {
                if child.kind() == "intersection_type" {
                    let (joined, t) = resolve_intersection(child, src, ctx);
                    arms.push(joined);
                    targets.extend(t);
                } else {
                    let (a, t) = resolve_type_node(child, src, ctx);
                    arms.extend(a);
                    targets.extend(t);
                }
            }
            (arms, targets)
        }
        _ => (vec![], vec![]),
    }
}

fn resolve_intersection(
    node: tree_sitter::Node,
    src: &[u8],
    ctx: &TypeCtx,
) -> (String, Vec<String>) {
    let mut members = Vec::new();
    let mut targets = Vec::new();
    for child in named_children(node) {
        let (arms, t) = resolve_type_node(child, src, ctx);
        members.push(arms.join("|"));
        targets.extend(t);
    }
    (format!("({})", members.join("&")), targets)
}

pub(crate) fn resolve_type(node: tree_sitter::Node, src: &[u8], ctx: &TypeCtx) -> ResolvedType {
    let (arms, named_targets) = resolve_type_node(node, src, ctx);
    ResolvedType {
        normalized: arms.join("|"),
        named_targets,
    }
}

fn dedup_targets(targets: &[String]) -> Vec<String> {
    let mut sorted: Vec<String> = targets.to_vec();
    sorted.sort();
    sorted.dedup();
    sorted
}

pub(crate) fn buffer_type_of_edges(
    emitter: &mut Emitter,
    source_id: &str,
    resolved: &ResolvedType,
) {
    for target in dedup_targets(&resolved.named_targets) {
        emitter.buffer_type_edge(source_id, EdgeLabel::TYPE_OF, &target);
    }
}

pub(crate) fn buffer_returns_edges(
    emitter: &mut Emitter,
    source_id: &str,
    resolved: &ResolvedType,
) {
    for target in dedup_targets(&resolved.named_targets) {
        emitter.buffer_type_edge(source_id, EdgeLabel::RETURNS, &target);
    }
}

pub(crate) fn resolve_return_type(
    fn_like: tree_sitter::Node,
    src: &[u8],
    ctx: &TypeCtx,
) -> Option<(String, ResolvedType)> {
    let return_type = fn_like.child_by_field_name("return_type")?;
    let path = text(return_type, src).unwrap_or_default().to_string();
    Some((path, resolve_type(return_type, src, ctx)))
}

pub(crate) fn base_clause_parent(
    class_like: tree_sitter::Node,
    src: &[u8],
    current_ns: Option<&str>,
    imports: &ImportTable,
) -> Option<String> {
    let mut cursor = class_like.walk();
    let base_clause = class_like
        .children(&mut cursor)
        .find(|c| c.kind() == "base_clause")?;
    let first = named_children(base_clause)
        .into_iter()
        .find(|c| matches!(c.kind(), "name" | "qualified_name"))?;
    let raw = text(first, src)?;
    Some(imports.resolve(raw, current_ns))
}

pub(crate) fn emit_params(
    formal_parameters: tree_sitter::Node,
    src: &[u8],
    fn_qname: &str,
    fn_item_id: &str,
    ctx: &TypeCtx,
    file: &str,
    emitter: &mut Emitter,
) {
    let mut index = 0usize;
    for child in named_children(formal_parameters) {
        if !matches!(
            child.kind(),
            "simple_parameter" | "variadic_parameter" | "property_promotion_parameter"
        ) {
            continue;
        }
        let Some(name) = child
            .child_by_field_name("name")
            .and_then(|n| variable_name_text(n, src))
        else {
            index += 1;
            continue;
        };

        let id = param_node_id(fn_qname, index);
        let mut node = Node::new(id.as_str(), Label::new(Label::PARAM))
            .with_prop("index", i64::try_from(index).unwrap_or(i64::MAX))
            .with_prop("is_self", false)
            .with_prop("name", name)
            .with_prop("parent_qname", fn_qname);

        if let Some(type_field) = child.child_by_field_name("type") {
            let resolved = resolve_type(type_field, src, ctx);
            node = node
                .with_prop("type_path", text(type_field, src).unwrap_or_default())
                .with_prop("type_normalized", resolved.normalized.as_str());
            buffer_type_of_edges(emitter, id.as_str(), &resolved);
        }

        emitter.emit_node(node);
        emitter.emit_edge(Edge::new(
            fn_item_id,
            id.as_str(),
            EdgeLabel::new(EdgeLabel::HAS_PARAM),
        ));
        attributes::emit_attributes(child, src, ctx.current_ns, ctx.imports, &id, file, emitter);
        index += 1;
    }
}

struct FieldEmission<'a> {
    name: &'a str,
    class_qname: &'a str,
    class_item_id: &'a str,
    index: usize,
    type_field: Option<tree_sitter::Node<'a>>,
    attrs_owner: tree_sitter::Node<'a>,
    file: &'a str,
}

fn emit_field(spec: FieldEmission, src: &[u8], ctx: &TypeCtx, emitter: &mut Emitter) {
    let id = field_node_id(spec.class_qname, spec.name);
    let mut node = Node::new(id.as_str(), Label::new(Label::FIELD))
        .with_prop("index", i64::try_from(spec.index).unwrap_or(i64::MAX))
        .with_prop("name", spec.name)
        .with_prop("parent_qname", spec.class_qname);

    if let Some(type_field) = spec.type_field {
        let resolved = resolve_type(type_field, src, ctx);
        node = node
            .with_prop("type_path", text(type_field, src).unwrap_or_default())
            .with_prop("type_normalized", resolved.normalized.as_str());
        buffer_type_of_edges(emitter, id.as_str(), &resolved);
    }

    emitter.emit_node(node);
    emitter.emit_edge(Edge::new(
        spec.class_item_id,
        id.as_str(),
        EdgeLabel::new(EdgeLabel::HAS_FIELD),
    ));
    attributes::emit_attributes(
        spec.attrs_owner,
        src,
        ctx.current_ns,
        ctx.imports,
        &id,
        spec.file,
        emitter,
    );
}

pub(crate) fn emit_property_fields(
    property_declaration: tree_sitter::Node,
    src: &[u8],
    class: (&str, &str),
    start_index: usize,
    ctx: &TypeCtx,
    file: &str,
    emitter: &mut Emitter,
) -> usize {
    let (class_qname, class_item_id) = class;
    let type_field = property_declaration.child_by_field_name("type");
    let mut index = start_index;
    for child in named_children(property_declaration) {
        if child.kind() != "property_element" {
            continue;
        }
        let Some(name) = child
            .child_by_field_name("name")
            .and_then(|n| variable_name_text(n, src))
        else {
            continue;
        };
        emit_field(
            FieldEmission {
                name,
                class_qname,
                class_item_id,
                index,
                type_field,
                attrs_owner: property_declaration,
                file,
            },
            src,
            ctx,
            emitter,
        );
        index += 1;
    }
    index
}

pub(crate) fn emit_promoted_fields(
    method: tree_sitter::Node,
    src: &[u8],
    class: (&str, &str),
    start_index: usize,
    ctx: &TypeCtx,
    file: &str,
    emitter: &mut Emitter,
) -> usize {
    let (class_qname, class_item_id) = class;
    let Some(params) = method.child_by_field_name("parameters") else {
        return start_index;
    };
    let mut index = start_index;
    for child in named_children(params) {
        if child.kind() != "property_promotion_parameter" {
            continue;
        }
        let Some(name) = child
            .child_by_field_name("name")
            .and_then(|n| variable_name_text(n, src))
        else {
            continue;
        };
        emit_field(
            FieldEmission {
                name,
                class_qname,
                class_item_id,
                index,
                type_field: child.child_by_field_name("type"),
                attrs_owner: child,
                file,
            },
            src,
            ctx,
            emitter,
        );
        index += 1;
    }
    index
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

    fn ctx<'a>(imports: &'a ImportTable) -> TypeCtx<'a> {
        TypeCtx {
            current_ns: Some("App"),
            imports,
            enclosing_class_qname: Some("App\\C"),
            enclosing_class_parent: Some("App\\P"),
        }
    }

    #[test]
    fn union_type_flattens_both_primitive_arms_in_source_order() {
        let (tree, src) = parse("<?php class C { public int|float $a; }");
        let type_node = find(tree.root_node(), "union_type").expect("union_type present");
        let imports = ImportTable::default();
        let resolved = resolve_type(type_node, &src, &ctx(&imports));
        assert_eq!(resolved.normalized, "int|float");
    }

    #[test]
    fn optional_type_contributes_the_inner_arm_and_a_trailing_null() {
        let (tree, src) = parse("<?php class C { public function m(?Port $p) {} }");
        let type_node = find(tree.root_node(), "optional_type").expect("optional_type present");
        let imports = ImportTable::default();
        let resolved = resolve_type(type_node, &src, &ctx(&imports));
        assert_eq!(resolved.normalized, "App\\Port|null");
    }

    #[test]
    fn intersection_type_joins_resolved_members_with_ampersand_parenthesised() {
        let (tree, src) = parse("<?php class C { public function m(\\Vendor\\X&Y $i) {} }");
        let type_node =
            find(tree.root_node(), "intersection_type").expect("intersection_type present");
        let imports = ImportTable::default();
        let resolved = resolve_type(type_node, &src, &ctx(&imports));
        assert_eq!(resolved.normalized, "(Vendor\\X&App\\Y)");
    }

    #[test]
    fn self_and_static_resolve_to_the_enclosing_class() {
        let (tree, src) = parse("<?php class C { public function m(): static {} }");
        let type_node = find(tree.root_node(), "named_type").expect("named_type present");
        let imports = ImportTable::default();
        let resolved = resolve_type(type_node, &src, &ctx(&imports));
        assert_eq!(resolved.normalized, "App\\C");
    }

    #[test]
    fn parent_resolves_to_the_enclosing_class_declared_base() {
        let (tree, src) = parse("<?php class C extends P { public function m(): parent {} }");
        let type_node = find(tree.root_node(), "named_type").expect("named_type present");
        let imports = ImportTable::default();
        let resolved = resolve_type(type_node, &src, &ctx(&imports));
        assert_eq!(resolved.normalized, "App\\P");
    }

    #[test]
    fn base_clause_parent_reads_the_first_extends_name() {
        let (tree, src) = parse("<?php class C extends P {}");
        let class_node =
            find(tree.root_node(), "class_declaration").expect("class_declaration present");
        let imports = ImportTable::default();
        let parent = base_clause_parent(class_node, &src, Some("App"), &imports);
        assert_eq!(parent, Some("App\\P".to_string()));
    }

    #[test]
    fn base_clause_parent_is_none_without_a_base_clause() {
        let (tree, src) = parse("<?php class C {}");
        let class_node =
            find(tree.root_node(), "class_declaration").expect("class_declaration present");
        let imports = ImportTable::default();
        let parent = base_clause_parent(class_node, &src, Some("App"), &imports);
        assert_eq!(parent, None);
    }
}

//! AST → cfdb fact emission for `cfdb-extractor-ts`.
//!
//! Walks the tree-sitter syntax tree (rooted at a TypeScript `program`
//! node) and emits `:Item` nodes plus `IN_CRATE` / `IN_MODULE` edges
//! per the closed-set mapping documented at the crate root. The
//! file-walking and workspace-detection layers live in `lib.rs`; this
//! module sees an already-parsed tree and the module/crate identifiers
//! the caller has resolved.

use std::collections::BTreeMap;

use cfdb_core::fact::{Edge, Node, PropValue, Props};
use cfdb_core::schema::{EdgeLabel, Label};
use tree_sitter::Node as TsNode;

use super::PRODUCER_NAME;

/// Walk the tree-sitter `program` root. We only handle top-level
/// declarations (children of `program`) and the `declaration` child
/// of `export_statement`. Nested declarations inside fn bodies, JSX
/// expressions, and module augmentation are out of scope for the MVP.
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
            );
        }
    }
}

/// Peel off an `export_statement` wrapper. Returns
/// `(declaration_node, is_exported)`. For non-export children the
/// pair is `(Some(child), false)` — non-exported top-level
/// declarations still produce items, just with private visibility.
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

/// Emit the `:Item` node + `IN_CRATE` + (optional) `IN_MODULE` edges
/// for one top-level declaration. Unknown declaration kinds (import
/// statements, ambient module blocks, etc.) are silently skipped — the
/// MVP only needs the five mapped kinds (interface / type alias /
/// class / function / const).
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
) {
    let (name, kind) = match decl.kind() {
        "interface_declaration" => (
            named_child_text(decl, "name", source),
            "trait", // TS interface → :Item.kind="trait" per crate-root mapping
        ),
        "type_alias_declaration" => (named_child_text(decl, "name", source), "type"),
        "class_declaration" | "abstract_class_declaration" => {
            (named_child_text(decl, "name", source), "struct")
        }
        "function_declaration" => (named_child_text(decl, "name", source), "fn"),
        "lexical_declaration" | "variable_declaration" => {
            // `const x = ...;` / `let x = ...;` / `var x = ...;` —
            // tree-sitter wraps the binding in `variable_declarator`.
            // Emit one `:Item.kind="const"` per declarator (multi-binding
            // lines `const a = 1, b = 2` produce two items). MVP collapses
            // let/var into const since the closed set has no separate
            // mutability marker; visibility carries the export bit.
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
    emit_item_node(
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
        emit_item_node(
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
) {
    let qname = format!("{crate_name}::{module_qpath}::{name}");
    let id = format!("item:{qname}");
    // tree-sitter's `Point.row` is 0-indexed; cfdb's `:Item.line`
    // contract is 1-indexed (matches `proc_macro2::Span::start().line`
    // on the Rust side — see cfdb-extractor lib.rs `:Item.line` doc).
    let line = (decl.start_position().row + 1) as i64;
    let visibility = if exported { "public" } else { "private" };

    let mut props = Props::new();
    props.insert("qname".into(), PropValue::Str(qname));
    props.insert("name".into(), PropValue::Str(name.to_string()));
    props.insert("kind".into(), PropValue::Str(kind.to_string()));
    props.insert("crate".into(), PropValue::Str(crate_name.to_string()));
    props.insert(
        "module_qpath".into(),
        PropValue::Str(module_qpath.to_string()),
    );
    props.insert("file".into(), PropValue::Str(rel_path.to_string()));
    props.insert("line".into(), PropValue::Int(line));
    props.insert("is_test".into(), PropValue::Bool(false));
    props.insert("visibility".into(), PropValue::Str(visibility.into()));
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

/// Read the text of the named child field (e.g. the `name` field on
/// `interface_declaration`) from the source bytes. Returns `None`
/// when the child is absent (anonymous class expressions, malformed
/// input) or the byte range is not valid UTF-8.
fn named_child_text(node: TsNode<'_>, field: &str, source: &[u8]) -> Option<String> {
    let child = node.child_by_field_name(field)?;
    let bytes = &source[child.byte_range()];
    std::str::from_utf8(bytes).ok().map(|s| s.to_string())
}

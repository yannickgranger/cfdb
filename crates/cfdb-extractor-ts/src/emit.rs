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
    pending_implements: &mut Vec<(String, String)>,
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

    // IMPLEMENTS (pass 1): buffer `(this class, each implemented interface text)`.
    // Only `class_heritage -> implements_clause` is walked; `extends_clause`
    // (inheritance) is excluded (RFC-045 §3.3 D3-a). Resolved name-wise to
    // in-workspace `:Item`s in pass 2 (`resolve_implements`).
    if let "class_declaration" | "abstract_class_declaration" = decl.kind() {
        buffer_implements_targets(decl, source, &id, pending_implements);
        // Method-level `:Item`s (RFC-045 45-D0) — the call-site anchor for 45-D.
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

    // Call sites in a top-level function body (RFC-045 45-D) — caller is the fn.
    // (Method-body call sites are walked inside `emit_class_methods` above.)
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
        // `const`/`let`/`var` can't carry an `implements` clause — the id is
        // unused (no IMPLEMENTS buffering for these kinds).
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
    // `ts_construct` disambiguates a class (an IMPLEMENTS source) from an
    // interface (its target) — both squash to closed-set `:Item.kind`s
    // (struct / trait). Only emitted on class/interface declarations; the
    // value is the tree-sitter node kind (RFC-045 45-B, analogue of PHP's
    // `php_construct`).
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

/// Read the text of the named child field (e.g. the `name` field on
/// `interface_declaration`) from the source bytes. Returns `None`
/// when the child is absent (anonymous class expressions, malformed
/// input) or the byte range is not valid UTF-8.
fn named_child_text(node: TsNode<'_>, field: &str, source: &[u8]) -> Option<String> {
    let child = node.child_by_field_name(field)?;
    let bytes = &source[child.byte_range()];
    std::str::from_utf8(bytes).ok().map(|s| s.to_string())
}

/// Buffer one pending `IMPLEMENTS` pair per interface reference in a class's
/// `class_heritage -> implements_clause`. Every NAMED child of the
/// `implements_clause` is a type reference; its whole byte-range text is the
/// reference text, which uniformly covers all shapes —
/// `type_identifier` (`I1`), `nested_type_identifier` (`ns.I2`),
/// `generic_type` (`Generic<T>` — type arguments NOT stripped, §6),
/// `intersection_type`/`union_type`/etc. (raw text). The sibling
/// `extends_clause` is never entered (inheritance is deferred, §3.3 D3-a).
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

/// Buffer one pending pair per NAMED child of an `implements_clause` (the
/// `implements` keyword and `,` separators are unnamed tokens, skipped).
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

/// Pass 2 — emit an `IMPLEMENTS` edge for each buffered `(class, ref_text)`
/// whose `ref_text` matches the `name` of exactly one in-workspace `:Item`.
///
/// TS `implements Bar` carries no namespace (resolution would need import
/// tracking), so the producer matches the reference text against `:Item.name`
/// across the workspace. A simple identifier (`I1`) resolves to the uniquely
/// named interface; a generic/qualified/composite reference (`Generic<T>`,
/// `ns.I2`, `A & B`) matches no bare name and is dropped (closed-world — no
/// synthetic node, no dangling edge, §4 I4). An ambiguous name (the same
/// identifier declared in two modules) is also dropped — a documented
/// limitation pending import-aware resolution. Every emitted edge carries
/// `resolver = "tree-sitter-typescript"`.
pub(crate) fn resolve_implements(
    pending: &[(String, String)],
    nodes: &[Node],
    edges: &mut Vec<Edge>,
) {
    let mut by_name: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for n in nodes {
        if n.label.as_str() != Label::ITEM {
            continue;
        }
        // Only interfaces (`trait`) and classes (`struct`) are valid
        // `implements` targets. Restricting the name map to type-kind items
        // keeps method `:Item`s (`kind:"fn"`, RFC-045 45-D0) and free
        // functions/consts from shadowing an interface name (which would make
        // resolution ambiguous and silently drop the edge).
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
            continue; // unresolved (0 matches) or ambiguous (>1) → no edge
        }
        let mut props = Props::new();
        props.insert(
            "resolver".into(),
            PropValue::Str("tree-sitter-typescript".into()),
        );
        edges.push(Edge {
            src: class_id.clone(),
            dst: ids[0].to_string(),
            label: EdgeLabel::new(EdgeLabel::IMPLEMENTS),
            props,
        });
    }
}

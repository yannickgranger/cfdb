//! Method-level `:Item` emission for `cfdb-extractor-ts`.
//!
//! This module descends a `class_body` and emits a method `:Item{kind:"fn"}`
//! for each method-bearing member:
//!
//! - `method_definition` — regular methods, getters (`get x()`), setters
//!   (`set x(v)`), `async`, generators (`*g()`), and `static` methods all
//!   parse as `method_definition`;
//! - `public_field_definition` whose `value` is an `arrow_function`
//!   (`foo = () => {}` — an arrow-assigned method).
//!
//! `abstract_method_signature` / `method_signature` / `index_signature`
//! (no body → no call sites) and non-arrow fields are skipped.
//!
//! The method qname is `{crate}::{module}::{Class}::{method}` (`::`-separated,
//! consistent with PHP's `\Ns\Class::m`). A getter and a setter for the same
//! property share that qname; the first wins (deduped) — a documented
//! collapse, since both anchor call sites to the same logical member.

use std::collections::{BTreeMap, BTreeSet};

use cfdb_core::fact::{build_item_props_common, Edge, Node, PropValue};
use cfdb_core::schema::{EdgeLabel, Label};
use tree_sitter::Node as TsNode;

use super::PRODUCER_NAME;

/// Descend `class_decl`'s `class_body` and emit a method `:Item` (+ `IN_CRATE`
/// / `IN_MODULE` edges) for every method member. `class_name` is the simple
/// name of the enclosing class (the qname infix).
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
                continue; // getter/setter (or duplicate) collapse — first wins
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
            // Call sites in the method body — caller is this method.
            crate::call_walker::walk_call_sites(member, source, &qname, rel_path, nodes, edges);
            // Record after use so the owned `qname` moves into the set — no
            // per-iteration clone (it is borrowed by the two emit calls above).
            seen.insert(qname);
        }
    }
}

/// The method name of a class-body member, or `None` when the member is not a
/// (body-carrying) method — `abstract_method_signature`, `method_signature`,
/// `index_signature`, a non-arrow `public_field_definition`, and structural
/// members all return `None`.
fn method_member_name(member: TsNode<'_>, source: &[u8]) -> Option<String> {
    match member.kind() {
        "method_definition" => field_text(member, "name", source),
        "public_field_definition" => {
            // Only arrow-assigned fields are methods (`foo = () => {}`).
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

    // `{qname, name, kind, crate}` from the shared owner; the TS method path
    // layers `module_qpath`, `file`, `line`, `is_test`, `visibility`, and
    // `language` — and (like the TS declaration path) emits NO
    // `bounded_context` (#478).
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

/// `:Item.visibility` for a method — the `accessibility_modifier`
/// (`public`/`private`/`protected`) child when present, else `public`
/// (TS members are public by default).
fn method_visibility(member: TsNode<'_>) -> String {
    let mut cursor = member.walk();
    for child in member.children(&mut cursor) {
        if child.kind() == "accessibility_modifier" {
            // The modifier's own text is the keyword (`private` etc.).
            return child.child(0).map_or("public", |kw| kw.kind()).to_string();
        }
    }
    "public".to_string()
}

/// Text of a named field (e.g. `name`) of a node, or `None` when absent /
/// non-UTF-8.
fn field_text(node: TsNode<'_>, field: &str, source: &[u8]) -> Option<String> {
    let child = node.child_by_field_name(field)?;
    std::str::from_utf8(&source[child.byte_range()])
        .ok()
        .map(|s| s.to_string())
}

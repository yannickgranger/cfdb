use std::collections::BTreeMap;

use crate::emitter::{Emitter, PendingCallSite};
use crate::imports::ImportTable;
use cfdb_core::schema::{ArgKind, RECEIVER_POSITION};

use crate::text;

const CONSTRUCTOR: &str = "__construct";

pub(crate) struct PendingArgument {
    pub position: u32,
    pub kind: ArgKind,
    pub source_text: String,
    pub line: i64,
    pub col: i64,
}

pub(crate) struct ClassifiedCall {
    pub callee_path: String,
    pub resolve_target: Option<String>,
    pub kind: &'static str,
}

pub(crate) struct CallScope<'a> {
    pub caller_qname: &'a str,
    pub enclosing_class_qname: Option<&'a str>,
    pub current_ns: Option<&'a str>,
    pub imports: &'a ImportTable,
    pub file: &'a str,
}

pub(crate) fn walk_call_sites(
    decl: tree_sitter::Node,
    src: &[u8],
    scope: &CallScope<'_>,
    emitter: &mut Emitter,
) {
    let mut cursor = decl.walk();
    let Some(body) = decl
        .children(&mut cursor)
        .find(|c| c.kind() == "compound_statement")
    else {
        return;
    };
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    visit(body, src, scope, &mut counts, emitter);
}

fn visit(
    node: tree_sitter::Node,
    src: &[u8],
    scope: &CallScope<'_>,
    counts: &mut BTreeMap<String, usize>,
    emitter: &mut Emitter,
) {
    if let Some(call) = classify_call(
        node,
        src,
        scope.current_ns,
        scope.imports,
        scope.enclosing_class_qname,
    ) {
        let ClassifiedCall {
            callee_path,
            resolve_target,
            kind,
        } = call;
        let idx = {
            let counter = counts.entry(callee_path.clone()).or_insert(0);
            let i = *counter;
            *counter += 1;
            i
        };
        emitter.buffer_call_site(PendingCallSite {
            id: format!("callsite:{}:{callee_path}:{idx}", scope.caller_qname),
            caller_qname: scope.caller_qname.to_string(),
            callee_path,
            file: scope.file.to_string(),
            line: (node.start_position().row + 1) as i64,
            resolve_target,
            kind,
            arguments: collect_arguments(node, src),
        });
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        visit(child, src, scope, counts, emitter);
    }
}

fn classify_call(
    node: tree_sitter::Node,
    src: &[u8],
    current_ns: Option<&str>,
    imports: &ImportTable,
    enclosing_class_qname: Option<&str>,
) -> Option<ClassifiedCall> {
    match node.kind() {
        "function_call_expression" => {
            let raw = text(node.child_by_field_name("function")?, src)?;
            Some(ClassifiedCall {
                callee_path: raw.to_string(),
                resolve_target: Some(imports.resolve(raw, current_ns)),
                kind: "call",
            })
        }
        "scoped_call_expression" => {
            let name = text(node.child_by_field_name("name")?, src)?;
            let scope = node.child_by_field_name("scope")?;
            let scope_text = text(scope, src)?;
            let (callee_path, resolve_target) = classify_scoped_call(
                scope.kind(),
                scope_text,
                name,
                current_ns,
                imports,
                enclosing_class_qname,
            );
            Some(ClassifiedCall {
                callee_path,
                resolve_target,
                kind: "call",
            })
        }
        "member_call_expression" | "nullsafe_member_call_expression" => {
            let name = text(node.child_by_field_name("name")?, src)?;
            Some(ClassifiedCall {
                callee_path: name.to_string(),
                resolve_target: None,
                kind: "call",
            })
        }
        "object_creation_expression" => {
            classify_construction(node, src, current_ns, imports, enclosing_class_qname)
        }
        _ => None,
    }
}

fn classify_construction(
    node: tree_sitter::Node,
    src: &[u8],
    current_ns: Option<&str>,
    imports: &ImportTable,
    enclosing_class_qname: Option<&str>,
) -> Option<ClassifiedCall> {
    let mut cursor = node.walk();
    let class_node = node.children(&mut cursor).find(|c| c.is_named())?;
    if !matches!(class_node.kind(), "name" | "qualified_name") {
        return None;
    }
    let written = text(class_node, src)?;
    let resolve_target = match written {
        "self" | "static" | "parent" => {
            let (_, target) = classify_scoped_call(
                "relative_scope",
                written,
                CONSTRUCTOR,
                current_ns,
                imports,
                enclosing_class_qname,
            );
            target
        }
        _ => Some(format!(
            "{}::{CONSTRUCTOR}",
            imports.resolve(written, current_ns)
        )),
    };
    Some(ClassifiedCall {
        callee_path: written.to_string(),
        resolve_target,
        kind: "new",
    })
}

fn classify_scoped_call(
    scope_kind: &str,
    scope_text: &str,
    name: &str,
    current_ns: Option<&str>,
    imports: &ImportTable,
    enclosing_class_qname: Option<&str>,
) -> (String, Option<String>) {
    match scope_kind {
        "relative_scope" => match (scope_text, enclosing_class_qname) {
            ("self" | "static", Some(cls)) => {
                let path = format!("{cls}::{name}");
                (path.clone(), Some(path))
            }
            _ => (format!("parent::{name}"), None),
        },
        "name" | "qualified_name" => {
            let class_qname = imports.resolve(scope_text, current_ns);
            (
                format!("{scope_text}::{name}"),
                Some(format!("{class_qname}::{name}")),
            )
        }
        _ => (name.to_string(), None),
    }
}

fn collect_arguments(node: tree_sitter::Node, src: &[u8]) -> Vec<PendingArgument> {
    let mut out = Vec::new();
    let mut position = 0u32;

    if matches!(
        node.kind(),
        "member_call_expression" | "nullsafe_member_call_expression"
    ) {
        if let Some(receiver) = node.child_by_field_name("object") {
            out.push(pending_argument(receiver, receiver, src, RECEIVER_POSITION));
            position = RECEIVER_POSITION + 1;
        }
    }

    let mut cursor = node.walk();
    let Some(arguments) = node.children(&mut cursor).find(|c| c.kind() == "arguments") else {
        return out;
    };

    let mut argument_cursor = arguments.walk();
    for wrapper in arguments
        .children(&mut argument_cursor)
        .filter(|c| c.kind() == "argument")
    {
        let Some(expr) = argument_expression(wrapper) else {
            continue;
        };
        out.push(pending_argument(wrapper, expr, src, position));
        position += 1;
    }
    out
}

fn argument_expression(wrapper: tree_sitter::Node) -> Option<tree_sitter::Node> {
    let label = wrapper.child_by_field_name("name");
    let modifier = wrapper.child_by_field_name("reference_modifier");
    let mut cursor = wrapper.walk();
    let expr = wrapper
        .children(&mut cursor)
        .filter(tree_sitter::Node::is_named)
        .find(|c| Some(*c) != label && Some(*c) != modifier)?;
    if expr.kind() == "variadic_unpacking" {
        let mut inner = expr.walk();
        return expr.children(&mut inner).find(tree_sitter::Node::is_named);
    }
    Some(expr)
}

fn pending_argument(
    span: tree_sitter::Node,
    expr: tree_sitter::Node,
    src: &[u8],
    position: u32,
) -> PendingArgument {
    let kind =
        if span.kind() == "argument" && span.child_by_field_name("reference_modifier").is_some() {
            ArgKind::Ref
        } else {
            classify_arg_kind(expr)
        };
    PendingArgument {
        position,
        kind,
        source_text: text(span, src).unwrap_or_default().to_string(),
        line: (span.start_position().row + 1) as i64,
        col: (span.start_position().column + 1) as i64,
    }
}

fn classify_arg_kind(expr: tree_sitter::Node) -> ArgKind {
    match expr.kind() {
        "string" | "encapsed_string" | "heredoc" | "nowdoc" | "integer" | "float" | "boolean"
        | "null" => ArgKind::Literal,
        "variable_name"
        | "name"
        | "qualified_name"
        | "member_access_expression"
        | "nullsafe_member_access_expression"
        | "scoped_property_access_expression"
        | "class_constant_access_expression" => ArgKind::Path,
        "member_call_expression" | "nullsafe_member_call_expression" | "scoped_call_expression" => {
            ArgKind::MethodCall
        }
        "function_call_expression" | "object_creation_expression" => ArgKind::Call,
        _ => ArgKind::Other,
    }
}

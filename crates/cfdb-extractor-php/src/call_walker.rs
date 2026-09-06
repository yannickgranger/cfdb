use std::collections::BTreeMap;

use crate::emitter::{Emitter, PendingCallSite};
use crate::imports::ImportTable;
use crate::text;

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
    if let Some((callee_path, resolve_target)) = classify_call(
        node,
        src,
        scope.current_ns,
        scope.imports,
        scope.enclosing_class_qname,
    ) {
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
) -> Option<(String, Option<String>)> {
    match node.kind() {
        "function_call_expression" => {
            let raw = text(node.child_by_field_name("function")?, src)?;
            Some((raw.to_string(), Some(imports.resolve(raw, current_ns))))
        }
        "scoped_call_expression" => {
            let name = text(node.child_by_field_name("name")?, src)?;
            let scope = node.child_by_field_name("scope")?;
            let scope_text = text(scope, src)?;
            Some(classify_scoped_call(
                scope.kind(),
                scope_text,
                name,
                current_ns,
                imports,
                enclosing_class_qname,
            ))
        }
        "member_call_expression" | "nullsafe_member_call_expression" => {
            let name = text(node.child_by_field_name("name")?, src)?;
            Some((name.to_string(), None))
        }
        _ => None,
    }
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

use std::collections::BTreeMap;

use crate::emitter::{Emitter, PendingCallSite};
use crate::{qualify, text};

pub(crate) fn walk_call_sites(
    decl: tree_sitter::Node,
    src: &[u8],
    caller_qname: &str,
    enclosing_class_qname: Option<&str>,
    current_ns: Option<&str>,
    file: &str,
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
    visit(
        body,
        src,
        caller_qname,
        enclosing_class_qname,
        current_ns,
        file,
        &mut counts,
        emitter,
    );
}

#[allow(clippy::too_many_arguments)]
fn visit(
    node: tree_sitter::Node,
    src: &[u8],
    caller_qname: &str,
    enclosing_class_qname: Option<&str>,
    current_ns: Option<&str>,
    file: &str,
    counts: &mut BTreeMap<String, usize>,
    emitter: &mut Emitter,
) {
    if let Some((callee_path, resolve_target)) =
        classify_call(node, src, current_ns, enclosing_class_qname)
    {
        let idx = {
            let counter = counts.entry(callee_path.clone()).or_insert(0);
            let i = *counter;
            *counter += 1;
            i
        };
        emitter.buffer_call_site(PendingCallSite {
            id: format!("callsite:{caller_qname}:{callee_path}:{idx}"),
            caller_qname: caller_qname.to_string(),
            callee_path,
            file: file.to_string(),
            line: (node.start_position().row + 1) as i64,
            resolve_target,
        });
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        visit(
            child,
            src,
            caller_qname,
            enclosing_class_qname,
            current_ns,
            file,
            counts,
            emitter,
        );
    }
}

fn classify_call(
    node: tree_sitter::Node,
    src: &[u8],
    current_ns: Option<&str>,
    enclosing_class_qname: Option<&str>,
) -> Option<(String, Option<String>)> {
    match node.kind() {
        "function_call_expression" => {
            let raw = text(node.child_by_field_name("function")?, src)?;
            Some((raw.to_string(), Some(resolve_qualified(raw, current_ns))))
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
            let class_qname = resolve_qualified(scope_text, current_ns);
            (
                format!("{scope_text}::{name}"),
                Some(format!("{class_qname}::{name}")),
            )
        }
        _ => (name.to_string(), None),
    }
}

fn resolve_qualified(raw: &str, current_ns: Option<&str>) -> String {
    match raw.strip_prefix('\\') {
        Some(absolute) => absolute.to_string(),
        None => qualify(current_ns, raw),
    }
}

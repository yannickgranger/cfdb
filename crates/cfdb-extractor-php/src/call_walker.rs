//! Recursive call-site body walk for the PHP producer.
//!
//! Descends a function/method body, emitting one buffered [`PendingCallSite`]
//! per call expression. The per-caller, per-`callee_path` occurrence counter
//! (reset per body) feeds the `callsite:{caller}:{callee_path}:{idx}` id —
//! the same scheme the Rust producer uses.
//!
//! Resolution follows the §3.4 PHP scope table:
//!
//! | form | `callee_path` | resolves to in-workspace `:Item`? |
//! |---|---|---|
//! | `foo()` | `foo` | yes iff `<ns>\foo` exists |
//! | `\Ns\foo()` | `\Ns\foo` | yes iff `Ns\foo` exists |
//! | `C::bar()` | `C::bar` | yes iff `<ns>\C::bar` exists |
//! | `self::bar()` / `static::bar()` | `<enclosing-class>::bar` | yes iff it exists |
//! | `parent::bar()` | `parent::bar` | no (no superclass edge this RFC) |
//! | `$x->foo()` / `$x?->foo()` | `foo` | no (method name only) |
//! | `$cls::foo()` (dynamic) | `foo` | no |
//!
//! `new MyClass()` (`object_creation_expression`) is NOT a call site;
//! the walk skips it but still recurses into its arguments.

use std::collections::BTreeMap;

use crate::emitter::{Emitter, PendingCallSite};
use crate::{qualify, text};

/// Walk the body of a fn/method `decl` (`function_definition` /
/// `method_declaration`), buffering a call site for every call expression in
/// it (and its nested bodies/closures). The occurrence counter is fresh per
/// body, so two calls to the same `callee_path` in one body get distinct ids.
/// Abstract methods / interface method signatures have no `compound_statement`
/// body (just `;`) — nothing to walk.
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

#[allow(clippy::too_many_arguments)] // call-site context is wide; threading it beats a struct here
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

    // Recurse unconditionally — catches nested calls (`foo(bar())`), calls
    // inside `new Foo(bar())` arguments, and calls inside closures/arrow
    // functions (attributed to the same enclosing named caller).
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

/// Classify a node as a call expression and return `(callee_path,
/// resolve_target)`, or `None` when the node is not a (modelled) call.
/// `resolve_target` is the in-workspace `:Item` qname to look up for `CALLS`,
/// or `None` when the call is unresolvable in principle.
fn classify_call(
    node: tree_sitter::Node,
    src: &[u8],
    current_ns: Option<&str>,
    enclosing_class_qname: Option<&str>,
) -> Option<(String, Option<String>)> {
    match node.kind() {
        // foo() / \Ns\foo() — free function, qualified against current ns.
        "function_call_expression" => {
            let raw = text(node.child_by_field_name("function")?, src)?;
            Some((raw.to_string(), Some(resolve_qualified(raw, current_ns))))
        }
        // C::bar() / self::bar() / static::bar() / parent::bar() / $cls::foo()
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
        // $x->foo() / $x?->foo() — instance dispatch, method name only,
        // never resolved (no receiver-type inference this RFC).
        "member_call_expression" | "nullsafe_member_call_expression" => {
            let name = text(node.child_by_field_name("name")?, src)?;
            Some((name.to_string(), None))
        }
        _ => None,
    }
}

/// Resolve a `scoped_call_expression` by its `scope` child kind.
fn classify_scoped_call(
    scope_kind: &str,
    scope_text: &str,
    name: &str,
    current_ns: Option<&str>,
    enclosing_class_qname: Option<&str>,
) -> (String, Option<String>) {
    match scope_kind {
        // self:: / static:: / parent::
        "relative_scope" => match (scope_text, enclosing_class_qname) {
            // self::/static:: bind to the enclosing (declaring) class at
            // syntactic scope. `static::` late-static-binding is NOT modelled
            // without HIR — treated as `self::` (RFC-045 §3.4 ddd R2 amend).
            ("self" | "static", Some(cls)) => {
                let path = format!("{cls}::{name}");
                (path.clone(), Some(path))
            }
            // parent:: has no superclass edge this RFC — unresolved.
            _ => (format!("parent::{name}"), None),
        },
        // C::bar() / \Ns\C::bar() — static call to a named class.
        "name" | "qualified_name" => {
            let class_qname = resolve_qualified(scope_text, current_ns);
            (
                format!("{scope_text}::{name}"),
                Some(format!("{class_qname}::{name}")),
            )
        }
        // $cls::foo() and other dynamic scopes — method name only, unresolved.
        _ => (name.to_string(), None),
    }
}

/// Qualify a class/function reference to its in-workspace qname: a leading
/// `\` makes it absolute (strip it); otherwise qualify against `current_ns`
/// exactly as `:Item` qnames are built. No `use`-import resolution (MVP).
fn resolve_qualified(raw: &str, current_ns: Option<&str>) -> String {
    match raw.strip_prefix('\\') {
        Some(absolute) => absolute.to_string(),
        None => qualify(current_ns, raw),
    }
}

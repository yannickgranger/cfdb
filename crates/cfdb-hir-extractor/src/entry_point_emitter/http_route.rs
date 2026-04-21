//! HTTP route detection for axum / actix-web route registrations.
//! See parent module docs for the detection contract.

use std::collections::BTreeMap;
use std::path::Path;

use cfdb_core::fact::{Edge, Node, PropValue};
use cfdb_core::qname::item_node_id;
use cfdb_core::schema::{EdgeLabel, Label};
use ra_ap_hir::db::HirDatabase;
use ra_ap_hir::{ModuleDef, PathResolution, Semantics};
use ra_ap_syntax::ast::{self, HasArgList, LiteralKind};

use super::HTTP_ROUTE_METHOD_NAMES;
use crate::call_site_emitter::function_qname;

/// Recognize `axum` / `actix-web` route-registration method calls and
/// emit one `:EntryPoint { kind: "http_route" }` per `(literal path,
/// resolvable handler)` pair. Dispatch shapes:
///
/// - **axum 2-arg:** `<router>.route|get|post|put|delete|patch|nest(
///   "/path", handler)` — arg1 is the literal path, arg2 is either a
///   bare handler path, a call expression whose callee is the handler
///   (`api_router()`), or (for actix) a `web::<method>().to(handler)`
///   chain.
/// - **actix resource chain:** `<resource>.route|to(web::<method>().to(
///   handler))` where `<resource>` is itself `web::resource("/path")`.
///   The path comes from the `web::resource` receiver; the handler
///   from the innermost `.to()` call.
///
/// Handlers that do not resolve to a concrete
/// `ModuleDef::Function` (closures, unresolved paths, trait methods
/// without a known impl) are skipped — AC-5 from issue #124 mandates
/// path-resolved handlers, not raw closure expressions.
pub(super) fn classify_http_route_method_call<DB>(
    sema: &Semantics<'_, DB>,
    method_call: &ast::MethodCallExpr,
    file_path: &Path,
    nodes: &mut Vec<Node>,
    edges: &mut Vec<Edge>,
) where
    DB: HirDatabase + Sized,
{
    let Some(method_name) = method_call.name_ref() else {
        return;
    };
    let name = method_name.text().to_string();
    if !HTTP_ROUTE_METHOD_NAMES.contains(&name.as_str()) {
        return;
    }

    let Some(arg_list) = method_call.arg_list() else {
        return;
    };
    let args: Vec<ast::Expr> = arg_list.args().collect();

    let Some((path_literal, handler_expr)) = extract_path_and_handler(method_call, &args, &name)
    else {
        return;
    };

    let Some(handler_qname) = resolve_handler_qname(sema, &handler_expr) else {
        return;
    };

    emit_http_route(nodes, edges, &handler_qname, &path_literal, file_path);
}

/// Return `(literal_path, handler_expr)` for a recognized shape, or
/// `None` if this method call is not a route registration.
///
/// Two shapes are accepted:
///
/// 1. **2-arg call, literal arg0:** `.route|get|post|...|nest("/p",
///    handler_expr)`. Path from arg0, handler from arg1.
/// 2. **1-arg call on `web::resource("/p")` receiver:** `.route|to(
///    inner)`. Path from the receiver's literal argument, handler
///    from the inner expression (which is usually itself a
///    `web::<method>().to(handler)` chain — digested in
///    [`resolve_handler_qname`]).
///
/// **False-positive discipline.** The HTTP verb names
/// (`get`/`post`/`put`/`delete`/`patch`) are also common map/cache
/// method names (`Port::put("BTC/USD", quote)`). To avoid matching
/// those, the literal path MUST start with `/` — the canonical shape
/// for axum + actix route paths. A key like `"BTC/USD"` does not
/// qualify. Empty paths are also rejected.
pub(super) fn extract_path_and_handler(
    method_call: &ast::MethodCallExpr,
    args: &[ast::Expr],
    method_name: &str,
) -> Option<(String, ast::Expr)> {
    let (path, handler_expr) = if args.len() == 2 {
        let p = string_literal_value(&args[0])?;
        (p, args[1].clone())
    } else if args.len() == 1 && (method_name == "route" || method_name == "to") {
        // Single-arg form — only accepted when the receiver is a
        // `web::resource("/p")` call.
        let p = receiver_resource_path(method_call)?;
        (p, args[0].clone())
    } else {
        return None;
    };

    if !is_url_path(&path) {
        return None;
    }
    Some((path, handler_expr))
}

/// `true` when `s` looks like an HTTP route path: starts with `/`.
/// Empty string is NOT a valid axum / actix path — both frameworks
/// require at least `/` for the root route. Excluding non-slash
/// literals filters out map-like `.put("BTC/USD", …)` false positives
/// verified on qbot-core during target dogfood (#124).
fn is_url_path(s: &str) -> bool {
    s.starts_with('/')
}

/// Walk back through the receiver chain of `method_call` looking for
/// a `web::resource("/p")` call expression. Returns its literal path
/// argument if found. This supports the actix `service(web::resource(
/// "/h").route(...))` pattern where the literal path is the receiver
/// of the `.route` / `.to` call, not an argument.
fn receiver_resource_path(method_call: &ast::MethodCallExpr) -> Option<String> {
    let mut expr = method_call.receiver()?;
    loop {
        match expr {
            ast::Expr::CallExpr(call) => {
                if call_ends_in(&call, "resource") {
                    let args: Vec<ast::Expr> = call
                        .arg_list()
                        .map(|al| al.args().collect())
                        .unwrap_or_default();
                    return args.first().and_then(string_literal_value);
                }
                return None;
            }
            ast::Expr::MethodCallExpr(inner) => {
                // Walk further up the method chain (e.g.
                // `web::resource("/p").route(...).route(...)`).
                expr = inner.receiver()?;
            }
            _ => return None,
        }
    }
}

/// `true` when `call`'s callee path has `segment` as its last path
/// segment (namespace-agnostic — accepts `resource`, `web::resource`,
/// `actix_web::web::resource`).
fn call_ends_in(call: &ast::CallExpr, segment: &str) -> bool {
    let Some(ast::Expr::PathExpr(path_expr)) = call.expr() else {
        return false;
    };
    let Some(path) = path_expr.path() else {
        return false;
    };
    path.segment()
        .and_then(|s| s.name_ref())
        .is_some_and(|nr| nr.text() == segment)
}

/// If `expr` is a string literal, return its decoded value.
fn string_literal_value(expr: &ast::Expr) -> Option<String> {
    let ast::Expr::Literal(lit) = expr else {
        return None;
    };
    match lit.kind() {
        LiteralKind::String(s) => s.value().ok().map(|cow| cow.into_owned()),
        _ => None,
    }
}

/// Dig through a handler expression to find a resolvable function
/// path. Handles three shapes:
///
/// - `ast::Expr::PathExpr` — the direct handler path (`my_handler`,
///   `routes::index`). Resolve via `Semantics::resolve_path`.
/// - `ast::Expr::CallExpr` — a call whose result is the handler
///   (`api_router()` for axum `.nest`). Treat the callee path as the
///   handler so `.nest("/api", api_router())` exposes `api_router`.
/// - `ast::Expr::MethodCallExpr` — a method chain (`web::get().to(
///   handler)`, common in actix). Drill to the innermost `.to()`
///   argument and resolve that.
///
/// Closures and unresolved paths return `None` — those do not emit an
/// `:EntryPoint` per AC-5.
pub(super) fn resolve_handler_qname<DB>(
    sema: &Semantics<'_, DB>,
    expr: &ast::Expr,
) -> Option<String>
where
    DB: HirDatabase + Sized,
{
    match expr {
        ast::Expr::PathExpr(path_expr) => resolve_path_to_fn_qname(sema, path_expr),
        ast::Expr::CallExpr(call) => {
            // `api_router()` — the callee is the path we care about.
            match call.expr()? {
                ast::Expr::PathExpr(path_expr) => resolve_path_to_fn_qname(sema, &path_expr),
                _ => None,
            }
        }
        ast::Expr::MethodCallExpr(inner) => {
            // Actix pattern: `web::get().to(handler)`. Walk down
            // method chains looking for a `.to(handler)` call with a
            // single path argument.
            resolve_handler_from_method_chain(sema, inner)
        }
        _ => None,
    }
}

/// Drill through a method chain searching for a `.to(handler_path)`
/// call. Returns the resolved handler qname from the first such call
/// encountered. Walks receivers and also args — actix chains like
/// `web::get().to(handler)` place the handler as the `to()` argument.
fn resolve_handler_from_method_chain<DB>(
    sema: &Semantics<'_, DB>,
    method_call: &ast::MethodCallExpr,
) -> Option<String>
where
    DB: HirDatabase + Sized,
{
    // If this is a `.to(...)` call, try to resolve its first arg.
    let method_name = method_call.name_ref()?.text().to_string();
    if method_name == "to" {
        let arg_list = method_call.arg_list()?;
        if let Some(ast::Expr::PathExpr(path_expr)) = arg_list.args().next() {
            if let Some(q) = resolve_path_to_fn_qname(sema, &path_expr) {
                return Some(q);
            }
        }
    }
    // Otherwise walk up the receiver chain.
    match method_call.receiver()? {
        ast::Expr::MethodCallExpr(inner) => resolve_handler_from_method_chain(sema, &inner),
        _ => None,
    }
}

/// Resolve a `PathExpr` to its `hir::Function` and derive the qname
/// via the canonical formula shared with `call_site_emitter`.
fn resolve_path_to_fn_qname<DB>(
    sema: &Semantics<'_, DB>,
    path_expr: &ast::PathExpr,
) -> Option<String>
where
    DB: HirDatabase + Sized,
{
    let path = path_expr.path()?;
    match sema.resolve_path(&path)? {
        PathResolution::Def(ModuleDef::Function(func)) => Some(function_qname(sema, func)),
        _ => None,
    }
}

/// Emit one `:EntryPoint { kind: "http_route" }` plus its `EXPOSES`
/// edge. Node id includes the literal path so multiple routes sharing
/// a handler (e.g. `.get("/a", h)` + `.post("/a", h)`, or two distinct
/// paths wired to the same fn) get distinct `:EntryPoint` rows.
pub(super) fn emit_http_route(
    nodes: &mut Vec<Node>,
    edges: &mut Vec<Edge>,
    handler_qname: &str,
    path_literal: &str,
    file_path: &Path,
) {
    let ep_id = format!("entrypoint:http_route:{handler_qname}:{path_literal}");
    let file_str = file_path.to_string_lossy().into_owned();

    let mut props = BTreeMap::new();
    props.insert("name".into(), PropValue::Str(path_literal.to_string()));
    props.insert("kind".into(), PropValue::Str("http_route".to_string()));
    props.insert(
        "handler_qname".into(),
        PropValue::Str(handler_qname.to_string()),
    );
    props.insert("file".into(), PropValue::Str(file_str));
    // Parameter JSON reserved for follow-up enrichment (HTTP method,
    // extractors, body shape). MVP emits the empty array to satisfy
    // the schema descriptor's `params: json` attr.
    props.insert("params".into(), PropValue::Str("[]".to_string()));

    nodes.push(Node {
        id: ep_id.clone(),
        label: Label::new(Label::ENTRY_POINT),
        props,
    });

    edges.push(Edge {
        src: ep_id,
        dst: item_node_id(handler_qname),
        label: EdgeLabel::new(EdgeLabel::EXPOSES),
        props: BTreeMap::new(),
    });
}

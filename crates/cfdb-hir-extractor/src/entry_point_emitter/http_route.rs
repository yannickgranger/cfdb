use std::collections::BTreeMap;
use std::path::Path;

use cfdb_core::fact::{Edge, Node, PropValue};
use cfdb_core::qname::{entrypoint_node_id, item_node_id_for_target, TargetDiscriminator};
use cfdb_core::schema::{EdgeLabel, Label};
use ra_ap_hir::db::HirDatabase;
use ra_ap_hir::{HasCrate, ModuleDef, PathResolution, Semantics};
use ra_ap_syntax::ast::{self, HasArgList, LiteralKind};

use super::HTTP_ROUTE_METHOD_NAMES;
use crate::call_site_emitter::function_qname;
use crate::target_map::EmitCtx;

pub(super) fn classify_http_route_method_call<DB>(
    sema: &Semantics<'_, DB>,
    ctx: &EmitCtx<'_>,
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

    let Some((handler_qname, handler_target)) = resolve_handler_qname(sema, ctx, &handler_expr)
    else {
        return;
    };

    emit_http_route(
        nodes,
        edges,
        &handler_qname,
        &handler_target,
        &path_literal,
        file_path,
    );
}

pub(super) fn extract_path_and_handler(
    method_call: &ast::MethodCallExpr,
    args: &[ast::Expr],
    method_name: &str,
) -> Option<(String, ast::Expr)> {
    let (path, handler_expr) = if args.len() == 2 {
        let p = string_literal_value(&args[0])?;
        (p, args[1].clone())
    } else if args.len() == 1 && (method_name == "route" || method_name == "to") {
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

fn is_url_path(s: &str) -> bool {
    s.starts_with('/')
}

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
                expr = inner.receiver()?;
            }
            _ => return None,
        }
    }
}

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

fn string_literal_value(expr: &ast::Expr) -> Option<String> {
    let ast::Expr::Literal(lit) = expr else {
        return None;
    };
    match lit.kind() {
        LiteralKind::String(s) => s.value().ok().map(|cow| cow.into_owned()),
        _ => None,
    }
}

pub(super) fn resolve_handler_qname<DB>(
    sema: &Semantics<'_, DB>,
    ctx: &EmitCtx<'_>,
    expr: &ast::Expr,
) -> Option<(String, TargetDiscriminator)>
where
    DB: HirDatabase + Sized,
{
    match expr {
        ast::Expr::PathExpr(path_expr) => resolve_path_to_fn_qname(sema, ctx, path_expr),
        ast::Expr::CallExpr(call) => match call.expr()? {
            ast::Expr::PathExpr(path_expr) => resolve_path_to_fn_qname(sema, ctx, &path_expr),
            _ => None,
        },
        ast::Expr::MethodCallExpr(inner) => resolve_handler_from_method_chain(sema, ctx, inner),
        _ => None,
    }
}

fn resolve_handler_from_method_chain<DB>(
    sema: &Semantics<'_, DB>,
    ctx: &EmitCtx<'_>,
    method_call: &ast::MethodCallExpr,
) -> Option<(String, TargetDiscriminator)>
where
    DB: HirDatabase + Sized,
{
    let method_name = method_call.name_ref()?.text().to_string();
    if method_name == "to" {
        let arg_list = method_call.arg_list()?;
        if let Some(ast::Expr::PathExpr(path_expr)) = arg_list.args().next() {
            if let Some(q) = resolve_path_to_fn_qname(sema, ctx, &path_expr) {
                return Some(q);
            }
        }
    }
    match method_call.receiver()? {
        ast::Expr::MethodCallExpr(inner) => resolve_handler_from_method_chain(sema, ctx, &inner),
        _ => None,
    }
}

fn resolve_path_to_fn_qname<DB>(
    sema: &Semantics<'_, DB>,
    ctx: &EmitCtx<'_>,
    path_expr: &ast::PathExpr,
) -> Option<(String, TargetDiscriminator)>
where
    DB: HirDatabase + Sized,
{
    let path = path_expr.path()?;
    match sema.resolve_path(&path)? {
        PathResolution::Def(ModuleDef::Function(func)) => Some((
            function_qname(sema, func),
            ctx.discriminator(sema.db, func.krate(sema.db)),
        )),
        _ => None,
    }
}

pub(super) fn emit_http_route(
    nodes: &mut Vec<Node>,
    edges: &mut Vec<Edge>,
    handler_qname: &str,
    handler_target: &TargetDiscriminator,
    path_literal: &str,
    file_path: &Path,
) {
    let handler_identity = handler_target.identity(handler_qname);
    let ep_id = format!(
        "{}:{path_literal}",
        entrypoint_node_id("http_route", &handler_identity)
    );
    let file_str = file_path.to_string_lossy().into_owned();

    let mut props = BTreeMap::new();
    props.insert("name".into(), PropValue::Str(path_literal.to_string()));
    props.insert("kind".into(), PropValue::Str("http_route".to_string()));
    props.insert(
        "handler_qname".into(),
        PropValue::Str(handler_qname.to_string()),
    );
    props.insert("file".into(), PropValue::Str(file_str));
    props.insert("params".into(), PropValue::Str("[]".to_string()));

    nodes.push(Node {
        id: ep_id.clone(),
        label: Label::new(Label::ENTRY_POINT),
        props,
    });

    edges.push(Edge {
        src: ep_id,
        dst: item_node_id_for_target(handler_qname, handler_target),
        label: EdgeLabel::new(EdgeLabel::EXPOSES),
        props: BTreeMap::new(),
    });
}

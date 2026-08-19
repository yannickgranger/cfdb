use std::collections::BTreeMap;
use std::path::Path;

use cfdb_core::fact::{Edge, Node, PropValue};
use ra_ap_hir::db::HirDatabase;
use ra_ap_hir::Semantics;
use ra_ap_syntax::ast::{self, AstNode, HasArgList};

use super::{emit, enclosing_fn_handler, resolve_handler_arg};
use crate::target_map::EmitCtx;

pub(super) fn try_emit_cron_job<DB>(
    sema: &Semantics<'_, DB>,
    ctx: &EmitCtx<'_>,
    call: &ast::CallExpr,
    file_path: &Path,
    nodes: &mut Vec<Node>,
    edges: &mut Vec<Edge>,
) where
    DB: HirDatabase + Sized,
{
    let Some(callee) = call.expr() else {
        return;
    };
    let ast::Expr::PathExpr(path_expr) = callee else {
        return;
    };
    let Some(path) = path_expr.path() else {
        return;
    };
    let Some((qualifier_last, tail_name)) = path_qualifier_and_last(&path) else {
        return;
    };
    if qualifier_last != "Job" {
        return;
    }
    if tail_name != "new_async" && tail_name != "new" {
        return;
    }

    let Some(arg_list) = call.arg_list() else {
        return;
    };
    let args: Vec<ast::Expr> = arg_list.args().collect();
    if args.len() < 2 {
        return;
    }
    let Some(cron_expr) = extract_string_literal(&args[0]) else {
        return;
    };

    let Some(handler) = enclosing_fn_handler(sema, ctx, call.syntax()) else {
        return;
    };
    let mut extra = BTreeMap::new();
    extra.insert("cron_expr".into(), PropValue::Str(cron_expr));
    emit(nodes, edges, &handler, "cron_job", file_path, Some(extra));
}

pub(super) fn try_emit_websocket<DB>(
    sema: &Semantics<'_, DB>,
    ctx: &EmitCtx<'_>,
    method_call: &ast::MethodCallExpr,
    file_path: &Path,
    nodes: &mut Vec<Node>,
    edges: &mut Vec<Edge>,
) where
    DB: HirDatabase + Sized,
{
    let Some(name_ref) = method_call.name_ref() else {
        return;
    };
    if name_ref.text() != "on_upgrade" {
        return;
    }
    let Some(arg_list) = method_call.arg_list() else {
        return;
    };
    let Some(first_arg) = arg_list.args().next() else {
        return;
    };

    let handler = resolve_handler_arg(sema, ctx, &first_arg)
        .or_else(|| enclosing_fn_handler(sema, ctx, method_call.syntax()));
    let Some(handler) = handler else {
        return;
    };

    emit(nodes, edges, &handler, "websocket", file_path, None);
}

fn path_qualifier_and_last(path: &ast::Path) -> Option<(String, String)> {
    let last_segment = path.segment()?;
    let last = last_segment.name_ref()?.text().to_string();
    let qualifier = path.qualifier()?;
    let qualifier_last = qualifier.segment()?.name_ref()?.text().to_string();
    Some((qualifier_last, last))
}

fn extract_string_literal(expr: &ast::Expr) -> Option<String> {
    let ast::Expr::Literal(lit) = expr else {
        return None;
    };
    match lit.kind() {
        ast::LiteralKind::String(s) => s.value().ok().map(|cow| cow.into_owned()),
        _ => None,
    }
}

use std::collections::BTreeMap;
use std::path::Path;

use cfdb_core::fact::{Edge, Node};
use ra_ap_hir::db::HirDatabase;
use ra_ap_hir::{Function, ModuleDef, PathResolution, Semantics};
use ra_ap_ide_db::line_index::LineIndex;
use ra_ap_syntax::ast::{self, AstNode, HasArgList};
use ra_ap_syntax::{SyntaxKind, TextSize};

use super::facts::{emit_argument_facts, emit_positional_args, emit_resolved_call};
use crate::target_map::EmitCtx;

#[allow(clippy::too_many_arguments)]
pub(super) fn walk_file<DB>(
    sema: &Semantics<'_, DB>,
    ctx: &EmitCtx<'_>,
    source_file: &ast::SourceFile,
    file_path: &Path,
    line_index: &LineIndex,
    counts: &mut BTreeMap<(String, String), usize>,
    nodes: &mut Vec<Node>,
    edges: &mut Vec<Edge>,
) where
    DB: HirDatabase + Sized,
{
    for descendant in source_file.syntax().descendants() {
        match descendant.kind() {
            SyntaxKind::METHOD_CALL_EXPR => {
                if let Some(method_call) = ast::MethodCallExpr::cast(descendant) {
                    emit_method_call(
                        sema,
                        ctx,
                        &method_call,
                        file_path,
                        line_index,
                        counts,
                        nodes,
                        edges,
                    );
                }
            }
            SyntaxKind::CALL_EXPR => {
                if let Some(call_expr) = ast::CallExpr::cast(descendant) {
                    emit_path_call(
                        sema, ctx, &call_expr, file_path, line_index, counts, nodes, edges,
                    );
                }
            }
            _ => {}
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_method_call<DB>(
    sema: &Semantics<'_, DB>,
    ctx: &EmitCtx<'_>,
    method_call: &ast::MethodCallExpr,
    file_path: &Path,
    line_index: &LineIndex,
    counts: &mut BTreeMap<(String, String), usize>,
    nodes: &mut Vec<Node>,
    edges: &mut Vec<Edge>,
) where
    DB: HirDatabase + Sized,
{
    let Some(callee_fn) = sema.resolve_method_call(method_call) else {
        return;
    };
    let offset: TextSize = method_call.syntax().text_range().start();
    let line = line_index.line_col(offset).line as usize + 1;
    let Some(cs_id) = emit_resolved_call(
        sema,
        ctx,
        method_call.syntax(),
        callee_fn,
        "method",
        file_path,
        line,
        counts,
        nodes,
        edges,
    ) else {
        return;
    };
    if let Some(receiver) = method_call.receiver() {
        emit_argument_facts(&cs_id, &receiver, 0, line_index, file_path, nodes, edges);
    }
    if let Some(arg_list) = method_call.arg_list() {
        emit_positional_args(&cs_id, &arg_list, 1, line_index, file_path, nodes, edges);
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_path_call<DB>(
    sema: &Semantics<'_, DB>,
    ctx: &EmitCtx<'_>,
    call_expr: &ast::CallExpr,
    file_path: &Path,
    line_index: &LineIndex,
    counts: &mut BTreeMap<(String, String), usize>,
    nodes: &mut Vec<Node>,
    edges: &mut Vec<Edge>,
) where
    DB: HirDatabase + Sized,
{
    let Some(callee_fn) = resolve_path_call(sema, call_expr) else {
        return;
    };
    let offset: TextSize = call_expr.syntax().text_range().start();
    let line = line_index.line_col(offset).line as usize + 1;
    let Some(cs_id) = emit_resolved_call(
        sema,
        ctx,
        call_expr.syntax(),
        callee_fn,
        "fn",
        file_path,
        line,
        counts,
        nodes,
        edges,
    ) else {
        return;
    };
    if let Some(arg_list) = call_expr.arg_list() {
        emit_positional_args(&cs_id, &arg_list, 0, line_index, file_path, nodes, edges);
    }
}

fn resolve_path_call<DB>(sema: &Semantics<'_, DB>, call_expr: &ast::CallExpr) -> Option<Function>
where
    DB: HirDatabase + Sized,
{
    let ast::Expr::PathExpr(path_expr) = call_expr.expr()? else {
        return None;
    };
    let path = path_expr.path()?;
    let PathResolution::Def(ModuleDef::Function(func)) = sema.resolve_path(&path)? else {
        return None;
    };
    Some(func)
}

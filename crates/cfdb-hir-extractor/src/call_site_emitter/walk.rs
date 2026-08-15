//! Syntax-tree traversal — walk every method-call and path-call
//! expression, dispatch on `SyntaxKind`, and resolve each arm. The
//! fact-building it delegates to `super::facts`.

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

/// Walk every method-call AND resolvable path-call expression in
/// `source_file`, resolve it, and emit facts if resolution succeeds.
///
/// `ast::MethodCallExpr` and `ast::CallExpr` are disjoint AST node
/// kinds (the grammar separates `receiver.method(args)` from
/// `expr(args)`), so a single `descendants()` walk that matches
/// both produces no duplicates. The receiver-method shape resolves
/// via `Semantics::resolve_method_call`; the path shape resolves
/// via `Semantics::resolve_path` after extracting the `PathExpr`
/// function expression from the `CallExpr`.
#[allow(clippy::too_many_arguments)] // walker plumbing — same sink shape as emit_resolved_call
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
    // Dispatch on `SyntaxKind` so only the matching branch casts —
    // `AstNode::cast` moves by value, and an `if let / else if` chain
    // on `cast(descendant.clone())` flagged as a clone-in-loop in
    // quality-metrics. Matching on kind first lets each branch consume
    // `descendant` directly. Same pattern as `entry_point_emitter`'s per-file registry dispatch.
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

/// Resolve and emit one `receiver.method(args)` call — the
/// [`SyntaxKind::METHOD_CALL_EXPR`] arm of [`walk_file`], extracted so the
/// walker stays flat (the inline form scored cognitive-58 / nesting-7).
#[allow(clippy::too_many_arguments)] // walker plumbing — same sink shape as emit_resolved_call
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
    // 1-indexed source line of the call expression — matches the syn
    // extractor's convention; receiver-token start mirrors syn for `foo\n .bar()`.
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
    // Receiver at position 0, explicit args at 1..N.
    if let Some(receiver) = method_call.receiver() {
        emit_argument_facts(&cs_id, &receiver, 0, line_index, file_path, nodes, edges);
    }
    if let Some(arg_list) = method_call.arg_list() {
        emit_positional_args(&cs_id, &arg_list, 1, line_index, file_path, nodes, edges);
    }
}

/// Resolve and emit one `path(args)` call — the [`SyntaxKind::CALL_EXPR`]
/// arm of [`walk_file`].
#[allow(clippy::too_many_arguments)] // walker plumbing — same sink shape as emit_resolved_call
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
    // Same 1-indexed convention; offset is the CallExpr start (covers
    // `Foo::bar(args)` from the first path segment through the close paren).
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
    // RFC-043 Slice A: positions 0..N for all args.
    if let Some(arg_list) = call_expr.arg_list() {
        emit_positional_args(&cs_id, &arg_list, 0, line_index, file_path, nodes, edges);
    }
}

/// Resolve an `ast::CallExpr` whose function expression is a
/// `PathExpr` to a concrete `hir::Function`. Returns `None` for:
/// - Non-path function expressions (closure calls, method calls
///   accidentally wrapped in extra parens, function-pointer
///   indirection).
/// - Paths that don't resolve via HIR (macro-expanded or out-of-scope
///   identifiers).
/// - Paths that resolve to something other than a function (constants,
///   type aliases, enum variants — the last is technically callable
///   but its semantics are construction, not function dispatch; out
///   of scope per #387 non-goals).
///
/// Same `Semantics::resolve_path` infrastructure used by
/// `crate::entry_point_emitter::resolve_handler_arg` (issue #124);
/// the resolution result type is identical so the match arm reads
/// the same way.
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

//! Non-HTTP entry-point detectors — `cron_job` (`Job::new_async` /
//! `Job::new`) and `websocket` (`.on_upgrade(...)`). See parent
//! module docs for the detection contract.

use std::collections::BTreeMap;
use std::path::Path;

use cfdb_core::fact::{Edge, Node, PropValue};
use ra_ap_hir::db::HirDatabase;
use ra_ap_hir::Semantics;
use ra_ap_syntax::ast::{self, AstNode, HasArgList};

use super::{emit, enclosing_fn_name_and_qname, resolve_handler_arg, HandlerTarget};

/// If `call` matches the `Job::new_async(<cron>, <closure>)` or
/// `Job::new(<cron>, <closure>)` shape, emit a `cron_job`
/// `:EntryPoint`. Returns early on any mismatch in structure or when
/// the enclosing fn qname cannot be resolved.
pub(super) fn try_emit_cron_job<DB>(
    sema: &Semantics<'_, DB>,
    call: &ast::CallExpr,
    file_path: &Path,
    nodes: &mut Vec<Node>,
    edges: &mut Vec<Edge>,
) where
    DB: HirDatabase + Sized,
{
    // Callee must be a path expression (not e.g. `foo()()` or a method
    // call receiver).
    let Some(callee) = call.expr() else {
        return;
    };
    let ast::Expr::PathExpr(path_expr) = callee else {
        return;
    };
    let Some(path) = path_expr.path() else {
        return;
    };
    // Require a qualifier segment (the `Job::` part) followed by a
    // `new_async`/`new` method ident. This eliminates the lone `new()`
    // false-positive surface.
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
    // Require: arg 0 = cron literal, arg 1 = closure/fn ref.
    if args.len() < 2 {
        return;
    }
    let Some(cron_expr) = extract_string_literal(&args[0]) else {
        return;
    };

    let Some((name, qname)) = enclosing_fn_name_and_qname(sema, call.syntax()) else {
        return;
    };
    let mut extra = BTreeMap::new();
    extra.insert("cron_expr".into(), PropValue::Str(cron_expr));
    emit(
        nodes,
        edges,
        qname,
        name,
        "cron_job",
        file_path,
        Some(extra),
    );
}

/// If `method_call` matches `<receiver>.on_upgrade(<handler>)`, emit
/// a `websocket` `:EntryPoint`. When `<handler>` is a path that
/// resolves via HIR to a named fn, the `EXPOSES` edge targets that
/// fn's qname; otherwise (closure / block / unresolved), it falls
/// back to the enclosing fn.
pub(super) fn try_emit_websocket<DB>(
    sema: &Semantics<'_, DB>,
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

    let handler = resolve_handler_arg(sema, &first_arg).or_else(|| {
        enclosing_fn_name_and_qname(sema, method_call.syntax())
            .map(|(name, qname)| HandlerTarget { name, qname })
    });
    let Some(HandlerTarget { name, qname }) = handler else {
        return;
    };

    emit(nodes, edges, qname, name, "websocket", file_path, None);
}

/// Return `(qualifier_last_segment, last_segment)` of a path with at
/// least one qualifier. For `Job::new_async` yields `("Job",
/// "new_async")`; for `JobScheduler::add` yields `("JobScheduler",
/// "add")`; for a bare `new` path with no qualifier yields `None`.
fn path_qualifier_and_last(path: &ast::Path) -> Option<(String, String)> {
    let last_segment = path.segment()?;
    let last = last_segment.name_ref()?.text().to_string();
    let qualifier = path.qualifier()?;
    let qualifier_last = qualifier.segment()?.name_ref()?.text().to_string();
    Some((qualifier_last, last))
}

/// Extract the literal string value of an expression when it is a
/// plain string literal. Returns `None` for any other expression
/// shape (variable, const, raw bytes, etc.) — cron schedules that
/// come from a `const CRON: &str = "..."` will not be captured by
/// this syntactic extractor; that is an accepted MVP limitation
/// tracked under the broader HIR-based literal-folding work.
fn extract_string_literal(expr: &ast::Expr) -> Option<String> {
    let ast::Expr::Literal(lit) = expr else {
        return None;
    };
    match lit.kind() {
        ast::LiteralKind::String(s) => s.value().ok().map(|cow| cow.into_owned()),
        _ => None,
    }
}

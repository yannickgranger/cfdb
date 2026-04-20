//! `extract_entry_points` — scan the HIR-loaded VFS and emit
//! `:EntryPoint` nodes + `EXPOSES` edges for the v0.2 kind vocabulary
//! (RFC-029 §A1.1). Two scan shapes coexist in a single pass:
//!
//! - **Attribute-level** (Issue #86): `cli_command` for `struct`/`enum`
//!   with `#[derive(Parser/Subcommand)]`; `mcp_tool` for `fn` with an
//!   attribute whose last path segment is `tool`.
//! - **Call-expression-level** (Issue #125): `cron_job` for
//!   `Job::new_async(<cron-lit>, ..)` / `Job::new(<cron-lit>, ..)` (also
//!   catches wrapped forms like `JobScheduler::add(Job::new_async(...))`
//!   because the inner `Job::new*` fires the emitter); `websocket` for
//!   `<expr>.on_upgrade(<handler>)`. `cron_job` stores the literal
//!   schedule in `cron_expr` and exposes the enclosing fn (closures have
//!   no qname). `websocket` resolves a named-fn handler via
//!   `Semantics::resolve_path` and otherwise falls back to the enclosing
//!   fn (same closure policy as `cron_job`).
//!
//! `http_route` is a sibling call-expression kind shipped in Issue #124;
//! the `scan_file` dispatch in this module is additive so the two slices
//! do not conflict. Clap/MCP detection is attribute-textual rather than
//! trait-resolution-based so it works on unbuilt source and on
//! struct-only derives. Cron/WS detection is call-expression-based
//! because neither crate defines a user-facing attribute — the handler
//! is always passed by value into a constructor.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use cfdb_core::fact::{Edge, Node, PropValue};
use cfdb_core::qname::{item_node_id, item_qname};
use cfdb_core::schema::{EdgeLabel, Label};
use ra_ap_edition::Edition;
use ra_ap_hir::db::HirDatabase;
use ra_ap_hir::{HasCrate, ModuleDef, PathResolution, Semantics};
use ra_ap_hir_ty::attach_db;
use ra_ap_syntax::ast::{self, AstNode, HasArgList, HasAttrs, HasName};
use ra_ap_syntax::{SyntaxKind, SyntaxNode};
use ra_ap_vfs::{Vfs, VfsPath};

use crate::error::HirError;

/// Extract entry-point facts from a loaded HIR database.
///
/// See module-level docs for the detection contract. Output is
/// sorted by node id (and edges by `(src, dst, label)`) before
/// return for G1 byte-stability.
///
/// # Errors
///
/// Returns [`HirError`] on VFS / parse failures. Individual items
/// whose qname cannot be resolved are silently skipped.
pub fn extract_entry_points<DB>(db: &DB, vfs: &Vfs) -> Result<(Vec<Node>, Vec<Edge>), HirError>
where
    DB: HirDatabase + Sized,
{
    attach_db(db, || extract_entry_points_attached(db, vfs))
}

fn extract_entry_points_attached<DB>(db: &DB, vfs: &Vfs) -> Result<(Vec<Node>, Vec<Edge>), HirError>
where
    DB: HirDatabase + Sized,
{
    let sema = Semantics::new(db);
    let mut nodes: Vec<Node> = Vec::new();
    let mut edges: Vec<Edge> = Vec::new();

    let mut files: Vec<(ra_ap_vfs::FileId, PathBuf)> = vfs
        .iter()
        .filter_map(|(file_id, vfs_path)| {
            let p = vfs_path_to_pathbuf(vfs_path)?;
            if p.extension().and_then(|e| e.to_str()) == Some("rs") {
                Some((file_id, p))
            } else {
                None
            }
        })
        .collect();
    files.sort_by(|a, b| a.1.cmp(&b.1));

    for (file_id, file_path) in files {
        let source_file = sema.parse_guess_edition(file_id);
        scan_file(&sema, &source_file, &file_path, &mut nodes, &mut edges);
    }

    nodes.sort_by(|a, b| a.id.cmp(&b.id));
    edges.sort_by(|a, b| {
        (a.src.as_str(), a.dst.as_str(), a.label.as_str()).cmp(&(
            b.src.as_str(),
            b.dst.as_str(),
            b.label.as_str(),
        ))
    });

    Ok((nodes, edges))
}

fn scan_file<DB>(
    sema: &Semantics<'_, DB>,
    source_file: &ast::SourceFile,
    file_path: &Path,
    nodes: &mut Vec<Node>,
    edges: &mut Vec<Edge>,
) where
    DB: HirDatabase + Sized,
{
    // Dispatch on `SyntaxKind` so only the matching branch casts
    // (`AstNode::cast` moves by value, which would require a clone
    // per branch in an `if let` chain; the metric scanner flags
    // repeated `.clone()` inside a loop even though `SyntaxNode`
    // clone is an `Rc` bump).
    for descendant in source_file.syntax().descendants() {
        match descendant.kind() {
            SyntaxKind::STRUCT => {
                if let Some(strukt) = ast::Struct::cast(descendant) {
                    if has_clap_derive(&strukt) {
                        if let Some((name, qname)) = struct_name_and_qname(sema, &strukt) {
                            emit(nodes, edges, qname, name, "cli_command", file_path, None);
                        }
                    }
                }
            }
            SyntaxKind::ENUM => {
                if let Some(enum_) = ast::Enum::cast(descendant) {
                    if has_clap_derive(&enum_) {
                        if let Some((name, qname)) = enum_name_and_qname(sema, &enum_) {
                            emit(nodes, edges, qname, name, "cli_command", file_path, None);
                        }
                    }
                }
            }
            SyntaxKind::FN => {
                if let Some(fn_ast) = ast::Fn::cast(descendant) {
                    if has_tool_attr(&fn_ast) {
                        if let Some((name, qname)) = fn_name_and_qname(sema, &fn_ast) {
                            emit(nodes, edges, qname, name, "mcp_tool", file_path, None);
                        }
                    }
                }
            }
            SyntaxKind::CALL_EXPR => {
                if let Some(call) = ast::CallExpr::cast(descendant) {
                    try_emit_cron_job(sema, &call, file_path, nodes, edges);
                }
            }
            SyntaxKind::METHOD_CALL_EXPR => {
                if let Some(mcall) = ast::MethodCallExpr::cast(descendant) {
                    try_emit_websocket(sema, &mcall, file_path, nodes, edges);
                }
            }
            _ => {}
        }
    }
}

/// `true` when the item's attribute list contains a `#[derive(...)]`
/// whose syntax text mentions `Parser` or `Subcommand`. Matching on
/// the raw syntax text handles `#[derive(Parser)]`, `#[derive(Parser,
/// Debug)]`, `#[derive(clap::Parser)]`, etc. uniformly.
fn has_clap_derive<N: HasAttrs>(item: &N) -> bool {
    item.attrs().any(|attr| {
        let text = attr.syntax().to_string();
        if !text.contains("derive") {
            return false;
        }
        text.contains("Parser") || text.contains("Subcommand")
    })
}

/// `true` when the fn carries an attribute whose last path segment
/// is `tool` (rmcp / mcp-core convention). Matches `#[tool]`,
/// `#[tool(...)]`, `#[rmcp::tool]`, etc.
fn has_tool_attr(fn_ast: &ast::Fn) -> bool {
    fn_ast.attrs().any(|attr| {
        let Some(path) = attr.meta().and_then(|m| m.path()) else {
            return false;
        };
        let last = path
            .syntax()
            .to_string()
            .rsplit("::")
            .next()
            .unwrap_or_default()
            .trim()
            .to_string();
        last == "tool"
    })
}

fn struct_name_and_qname<DB>(
    sema: &Semantics<'_, DB>,
    strukt: &ast::Struct,
) -> Option<(String, String)>
where
    DB: HirDatabase + Sized,
{
    let name = strukt.name()?.text().to_string();
    let def = sema.to_def(strukt)?;
    let qname = build_item_qname(sema, def.module(sema.db), def.krate(sema.db), &name);
    Some((name, qname))
}

fn enum_name_and_qname<DB>(sema: &Semantics<'_, DB>, enum_: &ast::Enum) -> Option<(String, String)>
where
    DB: HirDatabase + Sized,
{
    let name = enum_.name()?.text().to_string();
    let def = sema.to_def(enum_)?;
    let qname = build_item_qname(sema, def.module(sema.db), def.krate(sema.db), &name);
    Some((name, qname))
}

fn fn_name_and_qname<DB>(sema: &Semantics<'_, DB>, fn_ast: &ast::Fn) -> Option<(String, String)>
where
    DB: HirDatabase + Sized,
{
    let name = fn_ast.name()?.text().to_string();
    let def = sema.to_def(fn_ast)?;
    let qname = build_item_qname(sema, def.module(sema.db), def.krate(sema.db), &name);
    Some((name, qname))
}

/// If `call` matches the `Job::new_async(<cron>, <closure>)` or
/// `Job::new(<cron>, <closure>)` shape, emit a `cron_job`
/// `:EntryPoint`. Returns early on any mismatch in structure or when
/// the enclosing fn qname cannot be resolved.
fn try_emit_cron_job<DB>(
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
fn try_emit_websocket<DB>(
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

struct HandlerTarget {
    name: String,
    qname: String,
}

/// Resolve an argument expression to its `HandlerTarget` (name +
/// qname) when it is a path to a named fn. Closures, blocks, and
/// unresolved paths return `None` so the caller can fall back to the
/// enclosing-fn policy.
fn resolve_handler_arg<DB>(sema: &Semantics<'_, DB>, arg: &ast::Expr) -> Option<HandlerTarget>
where
    DB: HirDatabase + Sized,
{
    let ast::Expr::PathExpr(path_expr) = arg else {
        return None;
    };
    let path = path_expr.path()?;
    let resolution = sema.resolve_path(&path)?;
    let PathResolution::Def(ModuleDef::Function(func)) = resolution else {
        return None;
    };
    let name = func
        .name(sema.db)
        .display_no_db(Edition::Edition2021)
        .to_string();
    let qname = build_item_qname(sema, func.module(sema.db), func.krate(sema.db), &name);
    Some(HandlerTarget { name, qname })
}

/// Walk the syntax-tree ancestors of `node` looking for the
/// enclosing `fn` and return its `(name, qname)`. Used when the
/// registration call's handler argument has no own path-level qname
/// (closure) or when a cron schedule lives directly inside a fn.
fn enclosing_fn_name_and_qname<DB>(
    sema: &Semantics<'_, DB>,
    node: &SyntaxNode,
) -> Option<(String, String)>
where
    DB: HirDatabase + Sized,
{
    let fn_ast = node.ancestors().find_map(ast::Fn::cast)?;
    fn_name_and_qname(sema, &fn_ast)
}

/// Build `<crate>::<module_path>::<item_name>` via
/// `cfdb_core::qname::item_qname`. Shared by all kinds so cross-kind
/// IDs land on the same `:Item` node.
fn build_item_qname<DB>(
    _sema: &Semantics<'_, DB>,
    module: ra_ap_hir::Module,
    krate: ra_ap_hir::Crate,
    item_name: &str,
) -> String
where
    DB: HirDatabase + Sized,
{
    let db: &dyn HirDatabase = _sema.db;
    let crate_name = krate
        .display_name(db)
        .map(|n| n.to_string())
        .unwrap_or_default()
        .replace('-', "_");

    let mut stack: Vec<String> = module
        .path_to_root(db)
        .into_iter()
        .rev()
        .filter_map(|m| m.name(db))
        .map(|n| n.display_no_db(Edition::Edition2021).to_string())
        .collect();
    if !crate_name.is_empty() {
        stack.insert(0, crate_name);
    }

    item_qname(&stack, item_name)
}

/// Emit the `:EntryPoint` node and its `EXPOSES` edge. The optional
/// `extra_props` map is merged into the node props (e.g. `cron_expr`
/// for `cron_job`).
fn emit(
    nodes: &mut Vec<Node>,
    edges: &mut Vec<Edge>,
    handler_qname: String,
    display_name: String,
    kind: &str,
    file_path: &Path,
    extra_props: Option<BTreeMap<String, PropValue>>,
) {
    let ep_id = format!("entrypoint:{kind}:{handler_qname}");
    let file_str = file_path.to_string_lossy().into_owned();

    let mut props = BTreeMap::new();
    props.insert("name".into(), PropValue::Str(display_name));
    props.insert("kind".into(), PropValue::Str(kind.to_string()));
    props.insert(
        "handler_qname".into(),
        PropValue::Str(handler_qname.clone()),
    );
    props.insert("file".into(), PropValue::Str(file_str));
    // Parameter JSON is reserved for follow-up enrichment (extracting
    // clap arg shapes, MCP tool input schemas). MVP emits an empty
    // array to satisfy the schema descriptor's `params: json` attr.
    props.insert("params".into(), PropValue::Str("[]".to_string()));
    if let Some(extra) = extra_props {
        for (k, v) in extra {
            props.insert(k, v);
        }
    }

    nodes.push(Node {
        id: ep_id.clone(),
        label: Label::new(Label::ENTRY_POINT),
        props,
    });

    edges.push(Edge {
        src: ep_id,
        dst: item_node_id(&handler_qname),
        label: EdgeLabel::new(EdgeLabel::EXPOSES),
        props: BTreeMap::new(),
    });
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

fn vfs_path_to_pathbuf(p: &VfsPath) -> Option<PathBuf> {
    p.as_path().map(|abs| PathBuf::from(abs.as_str()))
}

//! `extract_entry_points` — scan the HIR-loaded VFS and emit
//! `:EntryPoint` nodes + `EXPOSES` edges for the v0.2 kind vocabulary
//! (RFC-029 §A1.1). Two scan shapes coexist in a single pass:
//!
//! - **Attribute-level** (Issue #86): `cli_command` for `struct`/`enum`
//!   with `#[derive(Parser/Subcommand)]`; `mcp_tool` for `fn` with an
//!   attribute whose last path segment is `tool`.
//! - **Call-expression-level** (Issues #124 + #125): `http_route` for
//!   `axum` `Router::route|get|post|put|delete|patch|nest("/path",
//!   handler)` and `actix_web` `.route("/p", web::<method>().to(h))` /
//!   `.service(web::resource("/p").route(...))` chains; `cron_job` for
//!   `Job::new_async(<cron-lit>, ..)` / `Job::new(<cron-lit>, ..)` (also
//!   catches wrapped forms like `JobScheduler::add(Job::new_async(...))`
//!   because the inner `Job::new*` fires the emitter); `websocket` for
//!   `<expr>.on_upgrade(<handler>)`. `http_route` resolves the handler
//!   via `Semantics::resolve_path`; `cron_job` stores the literal
//!   schedule in `cron_expr` and exposes the enclosing fn (closures have
//!   no qname); `websocket` resolves a named-fn handler and otherwise
//!   falls back to the enclosing fn.
//!
//! Clap/MCP detection is attribute-textual rather than trait-resolution
//! based so it works on unbuilt source and on struct-only derives.
//! Route/Cron/WS detection is call-expression-based because none of
//! those crates defines a user-facing attribute — the handler is always
//! passed by value into a constructor or method call.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use cfdb_core::fact::{Edge, Node, PropValue};
use cfdb_core::qname::{field_node_id, item_node_id, item_qname, param_node_id, variant_node_id};
use cfdb_core::schema::{EdgeLabel, Label};
use ra_ap_edition::Edition;
use ra_ap_hir::db::HirDatabase;
use ra_ap_hir::{HasCrate, ModuleDef, PathResolution, Semantics};
use ra_ap_hir_ty::attach_db;
use ra_ap_syntax::ast::{self, AstNode, HasAttrs, HasName};
use ra_ap_syntax::{SyntaxKind, SyntaxNode};
use ra_ap_vfs::{Vfs, VfsPath};

use crate::error::HirError;

mod http_route;
mod other_kinds;

/// HTTP method verbs recognized on axum's `Router` and actix's `App`.
/// `route` is overloaded (2-arg on axum / actix `App`, 1-arg on actix
/// `Resource`); `nest` is axum-specific sub-router composition. The
/// scan is method-name-syntactic — receiver type is not checked, which
/// keeps the fixture workspace free of real axum / actix dependencies
/// and matches the heuristic-not-annotation contract from
/// RFC-029 §A1.1 line 77.
const HTTP_ROUTE_METHOD_NAMES: &[&str] =
    &["route", "get", "post", "put", "delete", "patch", "nest"];

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
                            emit(
                                nodes,
                                edges,
                                qname.clone(),
                                name,
                                "cli_command",
                                file_path,
                                None,
                            );
                            // REGISTERS_PARAM for clap `#[derive(Parser)]`
                            // structs (#219 / RFC-037 §3.1, clap-struct row
                            // of the crate-ownership table). Walk the
                            // struct's named fields; for each one carrying
                            // `#[arg(...)]` emit a REGISTERS_PARAM edge
                            // pointing at the `:Field` node id the syn-side
                            // extractor produced via `field_node_id`. The
                            // HIR side does NOT emit `:Field` nodes itself —
                            // it relies on the syn-side producer to have
                            // emitted the node with the same canonical id
                            // (crate-ownership B9 resolution).
                            emit_clap_struct_registers_param(&qname, &strukt, edges);
                        }
                    }
                }
            }
            SyntaxKind::ENUM => {
                if let Some(enum_) = ast::Enum::cast(descendant) {
                    if has_clap_derive(&enum_) {
                        if let Some((name, qname)) = enum_name_and_qname(sema, &enum_) {
                            emit(
                                nodes,
                                edges,
                                qname.clone(),
                                name,
                                "cli_command",
                                file_path,
                                None,
                            );
                            // REGISTERS_PARAM for clap `#[derive(Subcommand)]`
                            // enums (#219 / RFC-037 §3.1, Subcommand row of
                            // the crate-ownership table). One edge per
                            // variant pointing at the `:Variant` node id the
                            // syn-side extractor produces via
                            // `variant_node_id`. Per-variant-field granularity
                            // is deferred to a follow-up RFC that introduces
                            // `:EntryPoint{kind:cli_subcommand}` (N1 —
                            // transitional approximation).
                            emit_clap_enum_registers_param(&qname, &enum_, edges);
                        }
                    }
                }
            }
            SyntaxKind::FN => {
                if let Some(fn_ast) = ast::Fn::cast(descendant) {
                    if has_tool_attr(&fn_ast) {
                        if let Some((name, qname)) = fn_name_and_qname(sema, &fn_ast) {
                            emit(
                                nodes,
                                edges,
                                qname.clone(),
                                name,
                                "mcp_tool",
                                file_path,
                                None,
                            );
                            // REGISTERS_PARAM for MCP `#[tool]` fns
                            // (#219 / RFC-037 §3.1 MCP row — HIR-owned).
                            // `ast::Fn` covers free fns AND impl methods;
                            // kept HIR-side because :EntryPoint is emitted
                            // here; syn-side emission would dangle src and
                            // be dropped by cfdb-petgraph's ingest.
                            emit_mcp_registers_param(&qname, &fn_ast, edges);
                        }
                    }
                }
            }
            SyntaxKind::CALL_EXPR => {
                if let Some(call) = ast::CallExpr::cast(descendant) {
                    other_kinds::try_emit_cron_job(sema, &call, file_path, nodes, edges);
                }
            }
            SyntaxKind::METHOD_CALL_EXPR => {
                if let Some(mcall) = ast::MethodCallExpr::cast(descendant) {
                    // Both detectors filter internally on method name
                    // (`on_upgrade` vs `route|get|post|...`), so the
                    // dispatch is O(n) and mutually exclusive in
                    // practice.
                    other_kinds::try_emit_websocket(sema, &mcall, file_path, nodes, edges);
                    http_route::classify_http_route_method_call(
                        sema, &mcall, file_path, nodes, edges,
                    );
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

/// `true` when a clap-struct field carries an `#[arg(...)]` (or bare
/// `#[arg]`) attribute — the clap convention for declaring a CLI-visible
/// input. Matches `#[arg]`, `#[arg(short, long)]`, `#[clap::arg]`, etc.
/// by checking the attribute path's last segment, mirroring `has_tool_attr`'s
/// discipline for multi-segment vs single-segment paths.
fn field_has_arg_attr(field: &ast::RecordField) -> bool {
    field.attrs().any(|attr| {
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
        last == "arg"
    })
}

/// Emit REGISTERS_PARAM edges for a clap `#[derive(Parser)]` struct —
/// one edge per `#[arg(...)]`-carrying named field, pointing at the
/// pre-existing `:Field` node id produced by the syn-side extractor via
/// `cfdb_core::qname::field_node_id`. Tuple structs and unit structs
/// emit zero edges; clap requires named fields on Parser structs.
///
/// The HIR side is deliberately edge-only: `:Field` nodes are owned by
/// the syn-side producer (RFC-037 §3.1 B9 — single-producer discipline
/// per structural node kind).
fn emit_clap_struct_registers_param(
    struct_qname: &str,
    strukt: &ast::Struct,
    edges: &mut Vec<Edge>,
) {
    let Some(ast::FieldList::RecordFieldList(record_list)) = strukt.field_list() else {
        // Tuple / unit structs — clap `#[derive(Parser)]` always uses
        // named fields, so any non-record field list has no `#[arg]`
        // fields to register.
        return;
    };
    let entry_point_id = format!("entrypoint:cli_command:{struct_qname}");
    for field in record_list.fields() {
        if !field_has_arg_attr(&field) {
            continue;
        }
        let Some(name) = field.name() else {
            continue;
        };
        let field_name = name.text().to_string();
        edges.push(Edge {
            src: entry_point_id.clone(),
            dst: field_node_id(struct_qname, &field_name),
            label: EdgeLabel::new(EdgeLabel::REGISTERS_PARAM),
            props: BTreeMap::new(),
        });
    }
}

/// Emit REGISTERS_PARAM edges for a clap `#[derive(Subcommand)]` enum —
/// one edge per declared variant, pointing at the pre-existing
/// `:Variant` node id produced by the syn-side extractor via
/// `cfdb_core::qname::variant_node_id`. Per-variant-field granularity
/// is explicitly deferred: §3.1 N1 documents the transitional
/// approximation (one edge per variant) that a future
/// `cli_subcommand` kind will supersede.
///
/// Variant index is the declaration order, matching `variant_node_id`'s
/// indexing policy.
fn emit_clap_enum_registers_param(enum_qname: &str, enum_: &ast::Enum, edges: &mut Vec<Edge>) {
    let Some(variant_list) = enum_.variant_list() else {
        return;
    };
    let entry_point_id = format!("entrypoint:cli_command:{enum_qname}");
    for (index, _variant) in variant_list.variants().enumerate() {
        edges.push(Edge {
            src: entry_point_id.clone(),
            dst: variant_node_id(enum_qname, index),
            label: EdgeLabel::new(EdgeLabel::REGISTERS_PARAM),
            props: BTreeMap::new(),
        });
    }
}

/// Emit one `REGISTERS_PARAM` edge per non-self param of an MCP
/// `#[tool]` fn (#219 / RFC-037 §3.1 MCP row — HIR-owned).
///
/// Targets the `:Param` node the syn extractor emits via
/// `param_node_id(fn_qname, index)`. Receiver-aware: when the fn has a
/// `self` / `&self` / `&mut self` receiver, the syn walker still calls
/// `emit_param` for it with `index=0`, so we offset the typed-param
/// index by 1 to match.
fn emit_mcp_registers_param(fn_qname: &str, fn_ast: &ast::Fn, edges: &mut Vec<Edge>) {
    let Some(param_list) = fn_ast.param_list() else {
        return;
    };
    let entry_point_id = format!("entrypoint:mcp_tool:{fn_qname}");
    let has_receiver = param_list.self_param().is_some();
    for (typed_index, _param) in param_list.params().enumerate() {
        let syn_index = if has_receiver {
            typed_index + 1
        } else {
            typed_index
        };
        edges.push(Edge {
            src: entry_point_id.clone(),
            dst: param_node_id(fn_qname, syn_index),
            label: EdgeLabel::new(EdgeLabel::REGISTERS_PARAM),
            props: BTreeMap::new(),
        });
    }
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

/// Resolve an `ast::Fn`'s qname via the canonical HIR fn-qname formula
/// in [`crate::call_site_emitter::function_qname`]. The formula is
/// impl-aware and trait-aware: methods inside `impl Foo { fn bar }` get
/// `<module>::Foo::bar`, trait impls get `<module>::Trait::bar`, free
/// fns get `<module>::bar`. Routing through the canonical builder
/// keeps cross-producer :Param / REGISTERS_PARAM keys bit-identical
/// with the syn-side emitter (RFC-037 §3.1 / #227).
fn fn_name_and_qname<DB>(sema: &Semantics<'_, DB>, fn_ast: &ast::Fn) -> Option<(String, String)>
where
    DB: HirDatabase + Sized,
{
    let name = fn_ast.name()?.text().to_string();
    let def = sema.to_def(fn_ast)?;
    let qname = crate::call_site_emitter::function_qname(sema, def);
    Some((name, qname))
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
    sema: &Semantics<'_, DB>,
    module: ra_ap_hir::Module,
    krate: ra_ap_hir::Crate,
    item_name: &str,
) -> String
where
    DB: HirDatabase + Sized,
{
    // RFC-029 §A1.2 object-safety constraint: the database is always
    // a monomorphic `DB: HirDatabase + Sized`. Passing `sema.db`
    // (which is `&DB`) directly to HIR query methods preserves the
    // monomorphisation — coercing to `&dyn HirDatabase` here was
    // pre-existing drift (no functional impact; all HIR query methods
    // accept `&impl HirDatabase`, not `&dyn HirDatabase`, because the
    // trait is not object-safe). AC-6 on #124 enforces zero `dyn
    // HirDatabase` anywhere under `crates/cfdb-hir-extractor/src/`.
    let db = sema.db;
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

fn vfs_path_to_pathbuf(p: &VfsPath) -> Option<PathBuf> {
    p.as_path().map(|abs| PathBuf::from(abs.as_str()))
}

//! `extract_entry_points` — scan the HIR-loaded VFS and emit
//! `:EntryPoint` nodes + `EXPOSES` edges for the v0.2 kind vocabulary
//! (RFC-029 §A1.1).
//!
//! Framework detection is dispatched through the [`framework`]
//! `FrameworkDetector` registry (RFC-049 §3.1, slice 49-0): each
//! framework recogniser is a registered detector, so adding a framework
//! is a registration rather than a new dispatch arm. Test/bench
//! classification is not a framework and runs as a dedicated pass
//! ([`scan_test_bench_fns`]). The registry refactor is recall-neutral —
//! the emitted fact set is byte-identical, guaranteed by the final sort
//! in [`extract_entry_points`]. The detector recogniser contract,
//! unchanged by 49-0, spans two scan shapes:
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
use cfdb_core::qname::{
    entrypoint_node_id, item_node_id_for_target, item_qname, TargetDiscriminator,
};
use cfdb_core::schema::{EdgeLabel, Label};
use ra_ap_edition::Edition;
use ra_ap_hir::db::HirDatabase;
use ra_ap_hir::{HasCrate, ModuleDef, PathResolution, Semantics};
use ra_ap_hir_ty::attach_db;
use ra_ap_syntax::ast::{self, AstNode, HasName};
use ra_ap_syntax::{SyntaxKind, SyntaxNode};
use ra_ap_vfs::{Vfs, VfsPath};

use crate::call_site_emitter::workspace_rs_files;
use crate::error::HirError;
use crate::target_map::{EmitCtx, TargetRootMap};

mod framework;
mod http_route;
mod other_kinds;
mod registers_param;
mod test_bench;

use framework::{FrameworkRegistry, Manifest};
use registers_param::has_tool_attr;
use test_bench::{has_bench_attr, has_test_attr, is_under_benches_dir, is_under_tests_dir};

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
/// `workspace_root` is the directory the workspace was loaded from; it
/// scopes the RFC-049 §3.1 manifest gate to the workspace's own member
/// crates (see [`framework::Manifest::from_crate_graph`]). The caller
/// passes the same path it handed to
/// [`build_hir_database`](crate::build_hir_database).
///
/// # Errors
///
/// Returns [`HirError`] on VFS / parse failures. Individual items
/// whose qname cannot be resolved are silently skipped.
pub fn extract_entry_points<DB>(
    db: &DB,
    vfs: &Vfs,
    workspace_root: &Path,
    targets: &TargetRootMap,
) -> Result<(Vec<Node>, Vec<Edge>), HirError>
where
    DB: HirDatabase + Sized,
{
    attach_db(db, || {
        extract_entry_points_attached(db, vfs, workspace_root, targets)
    })
}

fn extract_entry_points_attached<DB>(
    db: &DB,
    vfs: &Vfs,
    workspace_root: &Path,
    targets: &TargetRootMap,
) -> Result<(Vec<Node>, Vec<Edge>), HirError>
where
    DB: HirDatabase + Sized,
{
    let sema = Semantics::new(db);
    let ctx = EmitCtx { vfs, targets };
    let mut nodes: Vec<Node> = Vec::new();
    let mut edges: Vec<Edge> = Vec::new();

    // Workspace-scoped, workspace-relative enumeration shared with the
    // call-site emitter (#561 — see its walk-scope docs).
    let files = workspace_rs_files(vfs, workspace_root)?;

    let registry = FrameworkRegistry::<DB>::rust_default();
    // RFC-049 §3.1: populate the manifest from the workspace members'
    // own `[dependencies]` (scoped by `workspace_root`) so each
    // detector's `present(manifest)` gate consults them (49-A gates clap
    // on `clap`; 49-B gates the HTTP route detector on axum/actix).
    let manifest = Manifest::from_crate_graph(db, vfs, workspace_root);

    for (file_id, file_path) in files {
        let source_file = sema.parse_guess_edition(file_id);
        let (mut framework_nodes, mut framework_edges) =
            registry.detect_file(&manifest, &sema, &ctx, &source_file, &file_path);
        nodes.append(&mut framework_nodes);
        edges.append(&mut framework_edges);
        // Test/bench entry points are not a manifest-gated framework, so
        // they are classified in a dedicated pass that honours the
        // `#[tool]` precedence (RFC-042 §3.1).
        scan_test_bench_fns(
            &sema,
            &ctx,
            &source_file,
            &file_path,
            &mut nodes,
            &mut edges,
        );
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

/// Emit `test` / `bench` `:EntryPoint`s for the fns in `source_file`.
///
/// Test/bench classification is not a manifest-gated framework, so it
/// runs outside the [`framework`] `FrameworkDetector` registry (RFC-049
/// §3.1). It preserves the RFC-042 §3.1 precedence below `#[tool]`: a
/// `#[tool]` fn is emitted as `mcp_tool` by the MCP detector and MUST
/// NOT also be classified test/bench, so `#[tool]` fns are skipped here.
/// Below that, attribute-based `#[test]` / `#[bench]` wins over the
/// `tests/` / `benches/` file-location fallback (both resolved by
/// [`test_bench_kind`]). Exactly one `:EntryPoint` per fn is preserved
/// (no-duplicate invariant, RFC-042 §4).
fn scan_test_bench_fns<DB>(
    sema: &Semantics<'_, DB>,
    ctx: &EmitCtx<'_>,
    source_file: &ast::SourceFile,
    file_path: &Path,
    nodes: &mut Vec<Node>,
    edges: &mut Vec<Edge>,
) where
    DB: HirDatabase + Sized,
{
    for descendant in source_file.syntax().descendants() {
        if descendant.kind() != SyntaxKind::FN {
            continue;
        }
        let Some(fn_ast) = ast::Fn::cast(descendant) else {
            continue;
        };
        if has_tool_attr(&fn_ast) {
            continue;
        }
        if let Some(kind) = test_bench_kind(&fn_ast, file_path) {
            if let Some(handler) = fn_handler(sema, ctx, &fn_ast) {
                emit(nodes, edges, &handler, kind, file_path, None);
            }
        }
    }
}

/// Classify a non-`#[tool]` fn as `test` / `bench` / `None` per the
/// RFC-042 §3.1 precedence below `#[tool]`: attribute first, then
/// file-location.
fn test_bench_kind(fn_ast: &ast::Fn, file_path: &Path) -> Option<&'static str> {
    if has_test_attr(fn_ast) {
        Some("test")
    } else if has_bench_attr(fn_ast) {
        Some("bench")
    } else if is_under_tests_dir(file_path) {
        Some("test")
    } else if is_under_benches_dir(file_path) {
        Some("bench")
    } else {
        None
    }
}

fn struct_handler<DB>(
    sema: &Semantics<'_, DB>,
    ctx: &EmitCtx<'_>,
    strukt: &ast::Struct,
) -> Option<Handler>
where
    DB: HirDatabase + Sized,
{
    let name = strukt.name()?.text().to_string();
    let def = sema.to_def(strukt)?;
    let krate = def.krate(sema.db);
    let qname = build_item_qname(sema, def.module(sema.db), krate, &name);
    let target = ctx.discriminator(sema.db, krate);
    Some(Handler {
        name,
        qname,
        target,
    })
}

fn enum_handler<DB>(
    sema: &Semantics<'_, DB>,
    ctx: &EmitCtx<'_>,
    enum_: &ast::Enum,
) -> Option<Handler>
where
    DB: HirDatabase + Sized,
{
    let name = enum_.name()?.text().to_string();
    let def = sema.to_def(enum_)?;
    let krate = def.krate(sema.db);
    let qname = build_item_qname(sema, def.module(sema.db), krate, &name);
    let target = ctx.discriminator(sema.db, krate);
    Some(Handler {
        name,
        qname,
        target,
    })
}

/// Resolve an `ast::Fn`'s qname via the canonical HIR fn-qname formula
/// in [`crate::call_site_emitter::function_qname`]. The formula is
/// impl-aware and trait-aware: methods inside `impl Foo { fn bar }` get
/// `<module>::Foo::bar`, trait impls get `<module>::Trait::bar`, free
/// fns get `<module>::bar`. Routing through the canonical builder
/// keeps cross-producer :Param / REGISTERS_PARAM keys bit-identical
/// with the syn-side emitter (RFC-037 §3.1 / #227).
fn fn_handler<DB>(sema: &Semantics<'_, DB>, ctx: &EmitCtx<'_>, fn_ast: &ast::Fn) -> Option<Handler>
where
    DB: HirDatabase + Sized,
{
    let name = fn_ast.name()?.text().to_string();
    let def = sema.to_def(fn_ast)?;
    let qname = crate::call_site_emitter::function_qname(sema, def);
    let target = ctx.discriminator(sema.db, def.krate(sema.db));
    Some(Handler {
        name,
        qname,
        target,
    })
}

/// A resolved entry-point handler: display name, bare display qname,
/// and the RFC-054 target discriminator of the crate that owns it.
/// Ids derive from [`Handler::identity`]; the `handler_qname` prop
/// stays the bare qname (display props never carry the identity
/// suffix — RFC-054 §3.5.1).
struct Handler {
    name: String,
    qname: String,
    target: TargetDiscriminator,
}

impl Handler {
    /// The discriminated identity string ids derive from.
    fn identity(&self) -> std::borrow::Cow<'_, str> {
        self.target.identity(&self.qname)
    }
}

/// Resolve an argument expression to its [`Handler`] when it is a path
/// to a named fn. Closures, blocks, and unresolved paths return `None`
/// so the caller can fall back to the enclosing-fn policy.
fn resolve_handler_arg<DB>(
    sema: &Semantics<'_, DB>,
    ctx: &EmitCtx<'_>,
    arg: &ast::Expr,
) -> Option<Handler>
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
    let krate = func.krate(sema.db);
    let qname = build_item_qname(sema, func.module(sema.db), krate, &name);
    let target = ctx.discriminator(sema.db, krate);
    Some(Handler {
        name,
        qname,
        target,
    })
}

/// Walk the syntax-tree ancestors of `node` looking for the
/// enclosing `fn` and return its [`Handler`]. Used when the
/// registration call's handler argument has no own path-level qname
/// (closure) or when a cron schedule lives directly inside a fn.
fn enclosing_fn_handler<DB>(
    sema: &Semantics<'_, DB>,
    ctx: &EmitCtx<'_>,
    node: &SyntaxNode,
) -> Option<Handler>
where
    DB: HirDatabase + Sized,
{
    let fn_ast = node.ancestors().find_map(ast::Fn::cast)?;
    fn_handler(sema, ctx, &fn_ast)
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
    // Key the crate segment off the PACKAGE name (not the bin TARGET name
    // `display_name` yields) so the qname matches the syn extractor and the
    // EXPOSES edge lands on a real :Item (#517).
    let crate_name = crate::crate_name::crate_qname_prefix(db, krate);

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
/// for `cron_job`). Ids derive from the handler's discriminated
/// identity (RFC-054 54-C): two same-qname handlers in sibling bins
/// are distinct entry points, and the `EXPOSES` dst joins the syn
/// side's discriminated `:Item`. The `handler_qname` prop stays the
/// bare display qname (RFC-054 §3.5.1).
fn emit(
    nodes: &mut Vec<Node>,
    edges: &mut Vec<Edge>,
    handler: &Handler,
    kind: &str,
    file_path: &Path,
    extra_props: Option<BTreeMap<String, PropValue>>,
) {
    let ep_id = entrypoint_node_id(kind, &handler.identity());
    let file_str = file_path.to_string_lossy().into_owned();

    let mut props = BTreeMap::new();
    props.insert("name".into(), PropValue::Str(handler.name.clone()));
    props.insert("kind".into(), PropValue::Str(kind.to_string()));
    props.insert(
        "handler_qname".into(),
        PropValue::Str(handler.qname.clone()),
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
        dst: item_node_id_for_target(&handler.qname, &handler.target),
        label: EdgeLabel::new(EdgeLabel::EXPOSES),
        props: BTreeMap::new(),
    });
}

fn vfs_path_to_pathbuf(p: &VfsPath) -> Option<PathBuf> {
    p.as_path().map(|abs| PathBuf::from(abs.as_str()))
}

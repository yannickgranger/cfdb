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

const HTTP_ROUTE_METHOD_NAMES: &[&str] =
    &["route", "get", "post", "put", "delete", "patch", "nest"];

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

    let files = workspace_rs_files(vfs, workspace_root)?;

    let registry = FrameworkRegistry::<DB>::rust_default();
    let manifest = Manifest::from_crate_graph(db, vfs, workspace_root);

    for (file_id, file_path) in files {
        let source_file = sema.parse_guess_edition(file_id);
        let (mut framework_nodes, mut framework_edges) =
            registry.detect_file(&manifest, &sema, &ctx, &source_file, &file_path);
        nodes.append(&mut framework_nodes);
        edges.append(&mut framework_edges);
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

struct Handler {
    name: String,
    qname: String,
    target: TargetDiscriminator,
}

impl Handler {
    fn identity(&self) -> std::borrow::Cow<'_, str> {
        self.target.identity(&self.qname)
    }
}

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

fn build_item_qname<DB>(
    sema: &Semantics<'_, DB>,
    module: ra_ap_hir::Module,
    krate: ra_ap_hir::Crate,
    item_name: &str,
) -> String
where
    DB: HirDatabase + Sized,
{
    let db = sema.db;
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

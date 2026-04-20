//! `extract_entry_points` — attribute-based heuristic scanner that
//! emits `:EntryPoint` nodes and `EXPOSES` edges for clap CLI
//! commands and MCP tool handlers.
//!
//! ## Slice status — Issue #86 (MVP scope)
//!
//! Detects TWO entry-point kinds via syntax-level attribute matching:
//!
//! - `cli_command` — any `struct` or `enum` whose attribute list
//!   contains a `#[derive(...)]` whose syntax text mentions `Parser`
//!   or `Subcommand` (the canonical clap pattern). One `:EntryPoint`
//!   per detected item.
//! - `mcp_tool` — any `fn` item carrying an attribute whose last
//!   path segment is `tool` (rmcp / mcp-core convention). One
//!   `:EntryPoint` per annotated function.
//!
//! HTTP routes (`axum::Router::route("/path", handler)`) and cron
//! jobs (`tokio_cron_scheduler` registrations) are CALL-expression
//! level patterns that require a different scan shape; they are
//! deferred to follow-up issues. The v0.2-1 coverage gate measures
//! against MCP tools + CLI commands only (cfdb itself has zero MCP
//! tools today and a small number of clap command types).
//!
//! ## Why attribute-based, not HIR-type-based
//!
//! Detecting clap commands via "impl clap::Parser for X" would
//! require full trait-impl resolution and would miss struct-only
//! derives that haven't yet been monomorphised into impls at parse
//! time. The `#[derive(Parser)]` attribute is always textually
//! present at the item site — cheaper and more complete.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use cfdb_core::fact::{Edge, Node, PropValue};
use cfdb_core::qname::{item_node_id, item_qname};
use cfdb_core::schema::{EdgeLabel, Label};
use ra_ap_edition::Edition;
use ra_ap_hir::db::HirDatabase;
use ra_ap_hir::{HasCrate, Semantics};
use ra_ap_hir_ty::attach_db;
use ra_ap_syntax::ast::{self, AstNode, HasAttrs, HasName};
use ra_ap_syntax::SyntaxNode;
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
    source_file
        .syntax()
        .descendants()
        .for_each(|descendant| classify_descendant(sema, &descendant, file_path, nodes, edges));
}

/// Dispatch one syntax descendant into the matching `:EntryPoint` emission
/// path. Extracted from the walk in [`scan_file`] so the per-cast
/// `descendant.clone()` calls (required by `AstNode::cast` consuming its
/// argument) live in a helper rather than in the outer `for` loop body.
fn classify_descendant<DB>(
    sema: &Semantics<'_, DB>,
    descendant: &SyntaxNode,
    file_path: &Path,
    nodes: &mut Vec<Node>,
    edges: &mut Vec<Edge>,
) where
    DB: HirDatabase + Sized,
{
    if let Some(strukt) = ast::Struct::cast(descendant.clone()) {
        if has_clap_derive(&strukt) {
            if let Some((name, qname)) = struct_name_and_qname(sema, &strukt) {
                emit(nodes, edges, qname, name, "cli_command", file_path);
            }
        }
    } else if let Some(enum_) = ast::Enum::cast(descendant.clone()) {
        if has_clap_derive(&enum_) {
            if let Some((name, qname)) = enum_name_and_qname(sema, &enum_) {
                emit(nodes, edges, qname, name, "cli_command", file_path);
            }
        }
    } else if let Some(fn_ast) = ast::Fn::cast(descendant.clone()) {
        if has_tool_attr(&fn_ast) {
            if let Some((name, qname)) = fn_name_and_qname(sema, &fn_ast) {
                emit(nodes, edges, qname, name, "mcp_tool", file_path);
            }
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

/// Build `<crate>::<module_path>::<item_name>` via
/// `cfdb_core::qname::item_qname`.
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

/// Emit the `:EntryPoint` node and its `EXPOSES` edge.
fn emit(
    nodes: &mut Vec<Node>,
    edges: &mut Vec<Edge>,
    handler_qname: String,
    display_name: String,
    kind: &str,
    file_path: &Path,
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

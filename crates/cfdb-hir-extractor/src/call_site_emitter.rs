//! `extract_call_sites` — walk every source file in the VFS,
//! resolve method-call dispatch via `ra_ap_hir::Semantics`, emit
//! `:CallSite` + `CALLS(item→item)` + `INVOKES_AT(item→:CallSite)`
//! facts into cfdb-core's graph vocabulary.
//!
//! ## Slice status — Issue #85c (MVP)
//!
//! Handles method-call expressions (`foo.bar()`) resolvable to a
//! concrete `hir::Function` via `Semantics::resolve_method_call`.
//! Function-call expressions (`foo()`) and more exotic dispatches
//! (associated-function path calls, trait-object method calls with
//! no concrete impl) are deferred to follow-up issues.
//!
//! ## Cross-extractor ID stability
//!
//! Every `item:<qname>` ID is derived via `cfdb_core::qname` (the
//! canonical formula shipped in #90). Both the syn-based
//! `cfdb-extractor` and this HIR-based extractor produce bit-identical
//! qnames for the same source item — without that, `CALLS(item:A,
//! item:B)` edges from this extractor would silently dangle against
//! `:Item` nodes emitted by the syn extractor.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use cfdb_core::fact::{Edge, Node, PropValue};
use cfdb_core::qname::{item_node_id, item_qname, method_qname, normalize_impl_target};
use cfdb_core::schema::{EdgeLabel, Label};
use ra_ap_edition::Edition;
use ra_ap_hir::db::HirDatabase;
use ra_ap_hir::{
    AsAssocItem, AssocItemContainer, DisplayTarget, Function, HasCrate, HirDisplay, Semantics,
};
use ra_ap_hir_ty::attach_db;
use ra_ap_syntax::ast::{self, AstNode};
use ra_ap_syntax::SyntaxNode;
use ra_ap_vfs::{Vfs, VfsPath};

use crate::error::HirError;

/// Extract resolved call-site facts from a loaded HIR database.
///
/// Iterates every `.rs` file in `vfs`, parses each via
/// `Semantics::parse_guess_edition`, walks the syntax tree for method
/// calls, and resolves each via `Semantics::resolve_method_call`. Every
/// successful resolution emits exactly one `:CallSite` node (with
/// `resolver="hir"` + `callee_resolved=true`), one `CALLS(item:caller
/// → item:callee)` edge, and one `INVOKES_AT(item:caller →
/// :CallSite)` edge.
///
/// # Errors
///
/// Returns [`HirError`] on I/O or parsing failures. Individual method
/// calls that fail to resolve are silently skipped — an unresolved
/// call is not an error; it is simply data the HIR extractor cannot
/// claim resolution over. Note: this does NOT imply the syn extractor
/// has already seen the same call (syn may miss calls inside
/// macro-generated bodies that HIR can see but not resolve). The
/// semantic is "claim resolution only when precise" — HIR's
/// higher-precision / lower-recall tradeoff on generics and dynamic
/// dispatch is deliberate.
///
/// # Determinism
///
/// Output nodes and edges are sorted by ID before return, so two
/// invocations on the same workspace produce byte-identical vecs
/// regardless of the VFS iteration order chosen by `ra_ap_vfs`.
pub fn extract_call_sites<DB>(db: &DB, vfs: &Vfs) -> Result<(Vec<Node>, Vec<Edge>), HirError>
where
    DB: HirDatabase + Sized,
{
    // hir-ty's next-solver reads the database from its OWN thread-local
    // (separate from salsa's top-level attached slot). Without this
    // attach, any HIR query that dispatches through the solver panics
    // "Try to use attached db, but not db is attached". The closure
    // returns owned Vecs so the attach scope ends before we return.
    attach_db(db, || extract_call_sites_attached(db, vfs))
}

fn extract_call_sites_attached<DB>(db: &DB, vfs: &Vfs) -> Result<(Vec<Node>, Vec<Edge>), HirError>
where
    DB: HirDatabase + Sized,
{
    let sema = Semantics::new(db);
    let mut nodes: Vec<Node> = Vec::new();
    let mut edges: Vec<Edge> = Vec::new();

    // Collect files and sort by path for deterministic traversal.
    // The VFS iteration order is an implementation detail of salsa's
    // hash-set internals; sorting by path restores G1 byte-stability.
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
        // Per-call-site deduplication counter keyed by
        // `(caller_qname, callee_path)`.
        let mut counts: BTreeMap<(String, String), usize> = BTreeMap::new();
        walk_file(
            &sema,
            &source_file,
            &file_path,
            &mut counts,
            &mut nodes,
            &mut edges,
        );
    }

    // Stable sort: nodes by id, edges by (src, dst, label).
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

/// Walk every method-call expression in `source_file`, resolve it,
/// and emit facts if resolution succeeds.
fn walk_file<DB>(
    sema: &Semantics<'_, DB>,
    source_file: &ast::SourceFile,
    file_path: &Path,
    counts: &mut BTreeMap<(String, String), usize>,
    nodes: &mut Vec<Node>,
    edges: &mut Vec<Edge>,
) where
    DB: HirDatabase + Sized,
{
    for descendant in source_file.syntax().descendants() {
        if let Some(method_call) = ast::MethodCallExpr::cast(descendant.clone()) {
            if let Some(callee_fn) = sema.resolve_method_call(&method_call) {
                emit_resolved_call(
                    sema,
                    &method_call,
                    callee_fn,
                    file_path,
                    counts,
                    nodes,
                    edges,
                );
            }
        }
    }
}

/// Emit the three facts for one resolved method call.
fn emit_resolved_call<DB>(
    sema: &Semantics<'_, DB>,
    method_call: &ast::MethodCallExpr,
    callee: Function,
    file_path: &Path,
    counts: &mut BTreeMap<(String, String), usize>,
    nodes: &mut Vec<Node>,
    edges: &mut Vec<Edge>,
) where
    DB: HirDatabase + Sized,
{
    // Find the caller — the enclosing `fn` or method definition.
    let Some(caller_qname) = enclosing_fn_qname(sema, method_call.syntax()) else {
        return;
    };
    let callee_qname = function_qname(sema, callee);
    let callee_last_segment = callee_qname
        .rsplit("::")
        .next()
        .unwrap_or(&callee_qname)
        .to_string();

    let key = (caller_qname.clone(), callee_qname.clone());
    let idx = {
        let c = counts.entry(key).or_insert(0);
        let v = *c;
        *c += 1;
        v
    };
    let cs_id = format!("callsite:{caller_qname}:{callee_qname}:{idx}");
    let file_str = file_path.to_string_lossy().into_owned();

    let mut props = BTreeMap::new();
    props.insert("caller_qname".into(), PropValue::Str(caller_qname.clone()));
    props.insert("callee_path".into(), PropValue::Str(callee_qname.clone()));
    props.insert(
        "callee_last_segment".into(),
        PropValue::Str(callee_last_segment),
    );
    props.insert("kind".into(), PropValue::Str("method".to_string()));
    props.insert("file".into(), PropValue::Str(file_str));
    props.insert("line".into(), PropValue::Int(0));
    props.insert("is_test".into(), PropValue::Bool(false));
    props.insert("resolver".into(), PropValue::Str("hir".to_string()));
    props.insert("callee_resolved".into(), PropValue::Bool(true));

    nodes.push(Node {
        id: cs_id.clone(),
        label: Label::new(Label::CALL_SITE),
        props,
    });

    // CALLS (resolved): caller Item → callee Item.
    let mut calls_props = BTreeMap::new();
    calls_props.insert("resolved".into(), PropValue::Bool(true));
    edges.push(Edge {
        src: item_node_id(&caller_qname),
        dst: item_node_id(&callee_qname),
        label: EdgeLabel::new(EdgeLabel::CALLS),
        props: calls_props,
    });

    // INVOKES_AT: caller Item → :CallSite.
    edges.push(Edge {
        src: item_node_id(&caller_qname),
        dst: cs_id,
        label: EdgeLabel::new(EdgeLabel::INVOKES_AT),
        props: BTreeMap::new(),
    });
}

/// Walk the syntax-tree ancestors of `node` looking for the
/// enclosing `fn` (top-level or associated method). Returns its
/// qname if found.
fn enclosing_fn_qname<DB>(sema: &Semantics<'_, DB>, node: &SyntaxNode) -> Option<String>
where
    DB: HirDatabase + Sized,
{
    for ancestor in node.ancestors() {
        if let Some(fn_ast) = ast::Fn::cast(ancestor.clone()) {
            let fn_def = sema.to_def(&fn_ast)?;
            return Some(function_qname(sema, fn_def));
        }
    }
    None
}

/// Derive an `item:<qname>`-compatible qname for a `hir::Function`
/// using the canonical `cfdb_core::qname` formula. Both the syn and
/// HIR extractors share this formula so cross-extractor edges land
/// on the same Item node (DDD HIGH finding in #40 decomposition).
fn function_qname<DB>(sema: &Semantics<'_, DB>, func: Function) -> String
where
    DB: HirDatabase + Sized,
{
    let db = sema.db;
    let module_stack = build_module_stack(db, func);
    let fn_name = func
        .name(db)
        .display_no_db(Edition::Edition2021)
        .to_string();

    // If the function is an associated item inside an impl block,
    // produce `<module_qpath>::<impl_target>::<method>`. Else
    // `<module_qpath>::<fn_name>`. This mirrors cfdb-extractor's
    // item_visitor.rs derivation: method qnames interpose the impl
    // target between the enclosing module and the method name.
    if let Some(assoc) = AsAssocItem::as_assoc_item(func, db) {
        let display_target = DisplayTarget::from_crate(db, func.krate(db).into());
        match assoc.container(db) {
            AssocItemContainer::Impl(impl_block) => {
                // `HirDisplay` emits the fully monomorphised form
                // (`Vec<Node>`); `cfdb-extractor`'s syn renderer emits
                // the stripped form (`Vec`). Route through
                // `normalize_impl_target` so both extractors converge
                // on the same qname for `CALLS(Item→Item)` — #94 ddd
                // Q1 fix.
                let rendered = impl_block
                    .self_ty(db)
                    .display(db, display_target)
                    .to_string();
                let target = normalize_impl_target(&rendered);
                method_qname(&module_stack, &target, &fn_name)
            }
            AssocItemContainer::Trait(trait_def) => {
                let target = trait_def
                    .name(db)
                    .display_no_db(Edition::Edition2021)
                    .to_string();
                method_qname(&module_stack, &target, &fn_name)
            }
        }
    } else {
        item_qname(&module_stack, &fn_name)
    }
}

/// Build the module stack for a `hir::Function` — an ordered list
/// of module names from the crate root to (and including) the
/// enclosing module, with the crate name as the first element
/// (matching `cfdb-extractor/src/item_visitor.rs` convention).
fn build_module_stack<DB>(db: &DB, func: Function) -> Vec<String>
where
    DB: HirDatabase + Sized,
{
    let Some(module) = Some(func.module(db)) else {
        return Vec::new();
    };
    // `Module::path_to_root` returns the enclosing module followed
    // by every parent, ending at the crate root.
    let mut stack: Vec<String> = module
        .path_to_root(db)
        .into_iter()
        .rev()
        .filter_map(|m| m.name(db))
        .map(|n| n.display_no_db(Edition::Edition2021).to_string())
        .collect();

    // Root Module::name returns None for the crate root; prepend the
    // crate display name (underscores, matching Rust qname convention
    // the syn extractor uses).
    let krate = func.krate(db);
    // `CrateDisplayName::Display` impl renders the underscored
    // Rust-identifier form (matching cfdb-extractor's convention).
    let crate_name = krate
        .display_name(db)
        .map(|n| n.to_string())
        .unwrap_or_default();
    if !crate_name.is_empty() {
        // `path_to_root` does NOT include the crate root itself in
        // name-producing form; we insert it explicitly as element 0.
        stack.insert(0, crate_name.replace('-', "_"));
    }
    stack
}

/// Convert a `VfsPath` to a concrete filesystem path. In-memory
/// VFS paths (e.g. macro-expanded virtual files) return None.
fn vfs_path_to_pathbuf(p: &VfsPath) -> Option<PathBuf> {
    p.as_path().map(|abs| PathBuf::from(abs.as_str()))
}

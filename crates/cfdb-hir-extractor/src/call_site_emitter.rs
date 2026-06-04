//! `extract_call_sites` — walk every source file in the VFS,
//! resolve method-call AND path-call dispatch via `ra_ap_hir::Semantics`,
//! emit `:CallSite` + `CALLS(item→item)` + `INVOKES_AT(item→:CallSite)`
//! facts into cfdb-core's graph vocabulary.
//!
//! ## Resolved call shapes
//!
//! Two AST shapes produce resolved call facts:
//!
//! - **`ast::MethodCallExpr`** — `receiver.method(args)`. Resolved
//!   via `Semantics::resolve_method_call`. Wire `kind = "method"`.
//! - **`ast::CallExpr`** with a `PathExpr` function expression —
//!   plain function calls (`my_helper(args)`), associated-function
//!   calls (`MyType::new(args)`, `Foo::bar(args)`), and trait-static
//!   dispatch (`Trait::method(args)`) when statically resolvable
//!   to a concrete `hir::Function`. Resolved via
//!   `Semantics::resolve_path`. Wire `kind = "fn"` (the AST doesn't
//!   distinguish associated-function from free-fn at this site;
//!   downstream consumers can recheck via the resolved callee qname
//!   if they need the distinction). Issue #387 closed #85c's
//!   deferred follow-up scope.
//!
//! ## Out-of-scope dispatches
//!
//! Trait-object method calls (`(t: &dyn Trait).method()`) without
//! a static dispatch target, closure / function-pointer indirection,
//! and macro-expanded calls that ra-ap can't see through are still
//! deferred — they produce no resolved fact. Recall on these shapes
//! requires either type-inference precision the HIR doesn't currently
//! commit to, or a syn-side unresolved-CallSite parallel.
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
use cfdb_core::qname::{
    argument_node_id, item_node_id, item_qname, method_qname, normalize_impl_target,
};
use cfdb_core::schema::{EdgeLabel, Label};
use ra_ap_edition::Edition;
use ra_ap_hir::db::HirDatabase;
use ra_ap_hir::{
    AsAssocItem, AssocItemContainer, DisplayTarget, Function, HasCrate, HirDisplay, ModuleDef,
    PathResolution, Semantics,
};
use ra_ap_hir_ty::attach_db;
use ra_ap_ide_db::line_index::LineIndex;
use ra_ap_syntax::ast::{self, AstNode, HasArgList};
use ra_ap_syntax::{SyntaxKind, SyntaxNode, TextSize};
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
        // Build a LineIndex once per file so byte-offset → 1-indexed
        // source-line conversion in `walk_file` is O(log n) per call
        // site. The text comes from salsa-cached `SourceDatabase::file_text`,
        // so this read is free; constructing the LineIndex is a single
        // newline scan amortised across every method-call we emit. F-005
        // / EPIC #273: `:CallSite.line` in the HIR extractor was hardcoded
        // to 0 — this is the parity fix matching what PR #291 did for the
        // syn extractor.
        let file_text_handle = db.file_text(file_id);
        let file_text: &str = file_text_handle.text(db);
        let line_index = LineIndex::new(file_text);
        // Per-call-site deduplication counter keyed by
        // `(caller_qname, callee_path)`.
        let mut counts: BTreeMap<(String, String), usize> = BTreeMap::new();
        walk_file(
            &sema,
            &source_file,
            &file_path,
            &line_index,
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
fn walk_file<DB>(
    sema: &Semantics<'_, DB>,
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
    // `descendant` directly. Same pattern as `entry_point_emitter::scan_file`.
    for descendant in source_file.syntax().descendants() {
        match descendant.kind() {
            SyntaxKind::METHOD_CALL_EXPR => {
                if let Some(method_call) = ast::MethodCallExpr::cast(descendant) {
                    emit_method_call(
                        sema,
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
                        sema, &call_expr, file_path, line_index, counts, nodes, edges,
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
fn emit_method_call<DB>(
    sema: &Semantics<'_, DB>,
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
    // extractor's `proc_macro2::Span::start().line` convention (#291 /
    // F-005); receiver-token start mirrors syn for `foo\n .bar()`.
    let offset: TextSize = method_call.syntax().text_range().start();
    let line = line_index.line_col(offset).line as usize + 1;
    let Some(cs_id) = emit_resolved_call(
        sema,
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
    // RFC-043 Slice A: receiver at position 0, explicit args at 1..N.
    if let Some(receiver) = method_call.receiver() {
        emit_argument_facts(&cs_id, &receiver, 0, line_index, file_path, nodes, edges);
    }
    if let Some(arg_list) = method_call.arg_list() {
        emit_positional_args(&cs_id, &arg_list, 1, line_index, file_path, nodes, edges);
    }
}

/// Resolve and emit one `path(args)` call — the [`SyntaxKind::CALL_EXPR`]
/// arm of [`walk_file`].
fn emit_path_call<DB>(
    sema: &Semantics<'_, DB>,
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

/// Emit positional `:Argument` facts for an explicit arg list, numbering
/// from `base` (1 for method calls — the receiver occupies position 0 —
/// and 0 for path calls). Shared by [`emit_method_call`] and
/// [`emit_path_call`].
fn emit_positional_args(
    cs_id: &str,
    arg_list: &ast::ArgList,
    base: u32,
    line_index: &LineIndex,
    file_path: &Path,
    nodes: &mut Vec<Node>,
    edges: &mut Vec<Edge>,
) {
    for (i, arg) in arg_list.args().enumerate() {
        emit_argument_facts(
            cs_id,
            &arg,
            base + i as u32,
            line_index,
            file_path,
            nodes,
            edges,
        );
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

/// Emit the three facts for one resolved call. Shared by both the
/// method-call walker arm and the path-call walker arm in [`walk_file`].
///
/// `call_syntax` is the SyntaxNode of the call expression
/// (either an `ast::MethodCallExpr` or an `ast::CallExpr`) — used
/// only to locate the enclosing fn for the caller_qname; the
/// caller has already extracted the offset / line / resolved
/// callee.
///
/// `kind` is the wire-form discriminator stored as `:CallSite.kind`:
/// `"method"` for receiver-method calls, `"fn"` for path-call shapes
/// (free fn, associated fn, trait-static dispatch). Downstream
/// consumers that need to distinguish associated-function from
/// free-fn can re-derive the distinction from `callee_path`.
///
/// `line` is the 1-indexed source-line where the call expression
/// starts, computed by the caller from a per-file `LineIndex`.
/// Stored as `:CallSite.line` to match the syn extractor's wire
/// convention (#291 / F-005). A future synthetic or macro-expanded
/// span that produces no meaningful line should pass `0` (the
/// caller — `walk_file` — handles real source spans only, so today
/// every call here passes a real `line >= 1`).
#[allow(clippy::too_many_arguments)] // 9 args — :CallSite shape carries caller_qname, file, line, and kind as separate plumbed values per the syn extractor's emission signature; tying them into a struct would just shift the surface.
fn emit_resolved_call<DB>(
    sema: &Semantics<'_, DB>,
    call_syntax: &SyntaxNode,
    callee: Function,
    kind: &str,
    file_path: &Path,
    line: usize,
    counts: &mut BTreeMap<(String, String), usize>,
    nodes: &mut Vec<Node>,
    edges: &mut Vec<Edge>,
) -> Option<String>
where
    DB: HirDatabase + Sized,
{
    // Find the caller — the enclosing `fn` or method definition.
    let caller_qname = enclosing_fn_qname(sema, call_syntax)?;
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
    props.insert("kind".into(), PropValue::Str(kind.to_string()));
    props.insert("file".into(), PropValue::Str(file_str));
    props.insert("line".into(), PropValue::Int(line as i64));
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
        dst: cs_id.clone(),
        label: EdgeLabel::new(EdgeLabel::INVOKES_AT),
        props: BTreeMap::new(),
    });

    Some(cs_id)
}

/// Coarse syntactic classification of a `ra_ap_syntax::ast::Expr` into the
/// closed-set `kind` string used on `:Argument` nodes (RFC-043 §3.1 / §3.2).
///
/// HIR-native classifier — mirrors `cfdb_extractor_shared::classify_arg_kind`
/// but operates on `ra_ap_syntax::ast::Expr` rather than `syn::Expr` to avoid
/// adding `syn` as a runtime dep to `cfdb-hir-extractor`.
fn classify_hir_arg_kind(expr: &ast::Expr) -> &'static str {
    match expr {
        ast::Expr::PathExpr(_) => "path",
        ast::Expr::MethodCallExpr(_) => "method_call",
        ast::Expr::CallExpr(_) => "call",
        ast::Expr::RefExpr(_) => "ref",
        ast::Expr::Literal(_) => "literal",
        _ => "other",
    }
}

/// Emit one `:Argument` node and one `HAS_ARG` edge (RFC-043 Slice A).
///
/// `cs_id` — the owning `:CallSite` id.
/// `expr` — the `ra_ap_syntax` AST expression for the argument.
/// `position` — 0-indexed position; 0 = receiver for method calls.
#[allow(clippy::too_many_arguments)]
fn emit_argument_facts(
    cs_id: &str,
    expr: &ast::Expr,
    position: u32,
    line_index: &LineIndex,
    file_path: &Path,
    nodes: &mut Vec<Node>,
    edges: &mut Vec<Edge>,
) {
    let arg_id = argument_node_id(cs_id, position);
    let kind = classify_hir_arg_kind(expr);
    let source_text = expr.syntax().text().to_string();

    let offset = expr.syntax().text_range().start();
    let lc = line_index.line_col(offset);
    // LineIndex::line_col returns 0-indexed line/col; schema stores 1-indexed.
    let line = lc.line as i64 + 1;
    let col = lc.col as i64 + 1;

    let file_str = file_path.to_string_lossy().into_owned();

    let mut props = BTreeMap::new();
    props.insert("position".into(), PropValue::Int(i64::from(position)));
    props.insert("kind".into(), PropValue::Str(kind.to_string()));
    props.insert("source_text".into(), PropValue::Str(source_text));
    props.insert("file".into(), PropValue::Str(file_str));
    props.insert("line".into(), PropValue::Int(line));
    props.insert("col".into(), PropValue::Int(col));

    nodes.push(Node {
        id: arg_id.clone(),
        label: Label::new(Label::ARGUMENT),
        props,
    });
    edges.push(Edge {
        src: cs_id.to_string(),
        dst: arg_id,
        label: EdgeLabel::new(EdgeLabel::HAS_ARG),
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
    let fn_ast = node.ancestors().find_map(ast::Fn::cast)?;
    let fn_def = sema.to_def(&fn_ast)?;
    Some(function_qname(sema, fn_def))
}

/// Derive an `item:<qname>`-compatible qname for a `hir::Function`
/// using the canonical `cfdb_core::qname` formula. Both the syn and
/// HIR extractors share this formula so cross-extractor edges land
/// on the same Item node (DDD HIGH finding in #40 decomposition).
///
/// `pub(crate)` so [`crate::entry_point_emitter`] can reuse the same
/// formula when resolving `http_route` handler paths (Issue #124,
/// `ddd-specialist` gate: cross-kind ID stability — a handler fn
/// reached via `Semantics::resolve_path` must produce the same qname
/// as the same fn reached via `Semantics::resolve_method_call`).
pub(crate) fn function_qname<DB>(sema: &Semantics<'_, DB>, func: Function) -> String
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

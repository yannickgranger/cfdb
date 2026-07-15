//! Fact emission — build `:CallSite`, `CALLS`, `INVOKES_AT`, and
//! `:Argument`/`HAS_ARG` facts from resolved calls. Split out of
//! `call_site_emitter.rs` (#467).

use std::collections::BTreeMap;
use std::path::Path;

use cfdb_core::fact::{Edge, Node, PropValue};
use cfdb_core::qname::{argument_node_id, item_node_id};
use cfdb_core::schema::{EdgeLabel, Label};
use ra_ap_hir::db::HirDatabase;
use ra_ap_hir::{Function, Semantics};
use ra_ap_ide_db::line_index::LineIndex;
use ra_ap_syntax::ast::{self, AstNode};
use ra_ap_syntax::SyntaxNode;

use super::naming::{enclosing_fn_qname, function_qname};

/// Emit positional `:Argument` facts for an explicit arg list, numbering
/// from `base` (1 for method calls — the receiver occupies position 0 —
/// and 0 for path calls). Shared by [`emit_method_call`] and
/// [`emit_path_call`].
pub(super) fn emit_positional_args(
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
pub(super) fn emit_resolved_call<DB>(
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
pub(super) fn emit_argument_facts(
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

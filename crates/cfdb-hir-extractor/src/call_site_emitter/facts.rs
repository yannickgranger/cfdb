use std::collections::BTreeMap;
use std::path::Path;

use cfdb_core::fact::{Edge, Node, PropValue};
use cfdb_core::qname::{argument_node_id, callsite_node_id, item_node_id_for_target};
use cfdb_core::schema::{EdgeLabel, Label};
use ra_ap_hir::db::HirDatabase;
use ra_ap_hir::{Function, HasCrate, Semantics};
use ra_ap_ide_db::line_index::LineIndex;
use ra_ap_syntax::ast::{self, AstNode};
use ra_ap_syntax::SyntaxNode;

use super::naming::{enclosing_fn, function_qname};
use crate::target_map::{krate_discriminator, EmitCtx};

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

#[allow(clippy::too_many_arguments)]
pub(super) fn emit_resolved_call<DB>(
    sema: &Semantics<'_, DB>,
    ctx: &EmitCtx<'_>,
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
    let caller_fn = enclosing_fn(sema, call_syntax)?;
    let caller_qname = function_qname(sema, caller_fn);
    let callee_qname = function_qname(sema, callee);
    let callee_last_segment = callee_qname
        .rsplit("::")
        .next()
        .unwrap_or(&callee_qname)
        .to_string();

    let db = sema.db;
    let caller_target = krate_discriminator(db, ctx.vfs, ctx.targets, caller_fn.krate(db));
    let callee_target = krate_discriminator(db, ctx.vfs, ctx.targets, callee.krate(db));
    let caller_identity = caller_target.identity(&caller_qname);

    let key = (caller_identity.to_string(), callee_qname.clone());
    let idx = {
        let c = counts.entry(key).or_insert(0);
        let v = *c;
        *c += 1;
        v
    };
    let cs_id = callsite_node_id(&caller_identity, &callee_qname, idx);
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

    let mut calls_props = BTreeMap::new();
    calls_props.insert("resolved".into(), PropValue::Bool(true));
    edges.push(Edge {
        src: item_node_id_for_target(&caller_qname, &caller_target),
        dst: item_node_id_for_target(&callee_qname, &callee_target),
        label: EdgeLabel::new(EdgeLabel::CALLS),
        props: calls_props,
    });

    edges.push(Edge {
        src: item_node_id_for_target(&caller_qname, &caller_target),
        dst: cs_id.clone(),
        label: EdgeLabel::new(EdgeLabel::INVOKES_AT),
        props: BTreeMap::new(),
    });

    Some(cs_id)
}

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

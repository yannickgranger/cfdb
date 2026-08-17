//! Recursive call-site body walk for `cfdb-extractor-ts`.
//!
//! Descends a method/function declaration node and emits a `:CallSite`
//! (full Rust-parity prop set, `resolver = "tree-sitter-typescript"`) plus an
//! `INVOKES_AT` edge (`:Item{caller} -> :CallSite`) for every `call_expression`
//! in its body — including nested arrow-function bodies (calls there are
//! attributed to the enclosing named caller).
//!
//! `callee_path` is the whole text of the call's `function` field, which
//! uniformly covers all shapes: `identifier` (`foo`), `member_expression`
//! (`obj.foo`, `this.m`, `super.m`, optional `obj?.m` with `?.` preserved),
//! a nested `call_expression` (`a()` in `a()()`), and `parenthesized_expression`
//! (IIFE). A tagged template (`tag` `` `x` ``) is itself a `call_expression`
//! and emits a site; `new X()` is a `new_expression`, NOT a call site — the walk
//! skips it but still recurses into its arguments.
//!
//! **Zero `CALLS`.** TS resolves no import/receiver types (syn-parity), so
//! every `:CallSite` carries `callee_resolved = false` and no `CALLS` edge is
//! emitted. "Callers of X" is answered via `callee_path` + `INVOKES_AT`.

use std::collections::BTreeMap;

use cfdb_core::fact::{Edge, Node, PropValue, Props};
use cfdb_core::schema::{EdgeLabel, Label};
use tree_sitter::Node as TsNode;

/// Walk `decl` (a `method_definition` / `function_declaration` /
/// arrow-valued field) and emit a `:CallSite` + `INVOKES_AT` per
/// `call_expression`. The per-caller, per-`callee_path` occurrence counter is
/// fresh per body, so two calls to the same callee get distinct ids.
pub(crate) fn walk_call_sites(
    decl: TsNode<'_>,
    source: &[u8],
    caller_qname: &str,
    file: &str,
    nodes: &mut Vec<Node>,
    edges: &mut Vec<Edge>,
) {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    visit(decl, source, caller_qname, file, &mut counts, nodes, edges);
}

fn visit(
    node: TsNode<'_>,
    source: &[u8],
    caller_qname: &str,
    file: &str,
    counts: &mut BTreeMap<String, usize>,
    nodes: &mut Vec<Node>,
    edges: &mut Vec<Edge>,
) {
    if node.kind() == "call_expression" {
        if let Some(callee_path) = callee_path(node, source) {
            let idx = {
                let counter = counts.entry(callee_path.clone()).or_insert(0);
                let i = *counter;
                *counter += 1;
                i
            };
            emit_call_site(node, caller_qname, &callee_path, idx, file, nodes, edges);
        }
    }

    // Recurse unconditionally — catches nested calls (`a()()`), calls in
    // `new X(bar())` arguments, and calls inside arrow/closure bodies.
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        visit(child, source, caller_qname, file, counts, nodes, edges);
    }
}

/// The callee path of a `call_expression` — the whole text of its `function`
/// field (uniform across identifier / member / chained / parenthesized).
fn callee_path(call: TsNode<'_>, source: &[u8]) -> Option<String> {
    let function = call.child_by_field_name("function")?;
    std::str::from_utf8(&source[function.byte_range()])
        .ok()
        .map(|s| s.to_string())
}

fn emit_call_site(
    call: TsNode<'_>,
    caller_qname: &str,
    callee_path: &str,
    idx: usize,
    file: &str,
    nodes: &mut Vec<Node>,
    edges: &mut Vec<Edge>,
) {
    let id = format!("callsite:{caller_qname}:{callee_path}:{idx}");
    let line = (call.start_position().row + 1) as i64;

    let mut props = Props::new();
    props.insert("caller_qname".into(), PropValue::Str(caller_qname.into()));
    props.insert("callee_path".into(), PropValue::Str(callee_path.into()));
    props.insert(
        "callee_last_segment".into(),
        PropValue::Str(callee_last_segment(callee_path).into()),
    );
    props.insert("kind".into(), PropValue::Str("call".into()));
    props.insert("file".into(), PropValue::Str(file.into()));
    props.insert("line".into(), PropValue::Int(line));
    props.insert("is_test".into(), PropValue::Bool(false));
    props.insert(
        "resolver".into(),
        PropValue::Str("tree-sitter-typescript".into()),
    );
    // No import / receiver-type resolution (syn-parity) → never resolved,
    // never a CALLS edge.
    props.insert("callee_resolved".into(), PropValue::Bool(false));

    nodes.push(Node {
        id: id.clone(),
        label: Label::new(Label::CALL_SITE),
        props,
    });
    // INVOKES_AT: Item(caller) -> CallSite (the direction corrected in 45-C).
    edges.push(Edge {
        src: format!("item:{caller_qname}"),
        dst: id,
        label: EdgeLabel::new(EdgeLabel::INVOKES_AT),
        props: BTreeMap::new(),
    });
}

/// Last segment of a TS `callee_path` — the part after the final `.` member
/// separator (`obj.foo` → `foo`, `this.m` → `m`, `foo` → `foo`).
fn callee_last_segment(callee_path: &str) -> &str {
    callee_path.rsplit('.').next().unwrap_or(callee_path)
}

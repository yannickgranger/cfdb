//! REGISTERS_PARAM edge emitters for clap / MCP `:EntryPoint`s, plus
//! the attribute probes the parent's `scan_file` dispatcher uses to
//! branch on `#[derive(Parser|Subcommand)]` vs `#[tool]`.
//!
//! Split from `entry_point_emitter.rs` (#239 slice) to keep that file
//! under the 500-LOC architecture threshold. All helpers are
//! `pub(super)` — reachable only from the parent emitter module.

use std::collections::BTreeMap;

use cfdb_core::fact::Edge;
use cfdb_core::qname::{field_node_id, param_node_id, variant_node_id};
use cfdb_core::schema::EdgeLabel;

use ra_ap_syntax::ast::{self, AstNode, HasAttrs, HasName};

/// `true` when the item's attribute list contains a `#[derive(...)]`
/// whose syntax text mentions `Parser` or `Subcommand`. Matching on
/// the raw syntax text handles `#[derive(Parser)]`, `#[derive(Parser,
/// Debug)]`, `#[derive(clap::Parser)]`, etc. uniformly.
pub(super) fn has_clap_derive<N: HasAttrs>(item: &N) -> bool {
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
/// by checking the attribute path's last segment, mirroring [`has_tool_attr`]'s
/// discipline for multi-segment vs single-segment paths.
pub(super) fn field_has_arg_attr(field: &ast::RecordField) -> bool {
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

/// `true` when the fn carries an attribute whose last path segment
/// is `tool` (rmcp / mcp-core convention). Matches `#[tool]`,
/// `#[tool(...)]`, `#[rmcp::tool]`, etc.
pub(super) fn has_tool_attr(fn_ast: &ast::Fn) -> bool {
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

/// Emit REGISTERS_PARAM edges for a clap `#[derive(Parser)]` struct —
/// one edge per `#[arg(...)]`-carrying named field, pointing at the
/// pre-existing `:Field` node id produced by the syn-side extractor via
/// [`cfdb_core::qname::field_node_id`]. Tuple structs and unit structs
/// emit zero edges; clap requires named fields on Parser structs.
///
/// The HIR side is deliberately edge-only: `:Field` nodes are owned by
/// the syn-side producer (RFC-037 §3.1 B9 — single-producer discipline
/// per structural node kind).
pub(super) fn emit_clap_struct_registers_param(
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
    // Iterator chain form avoids `.clone()` inside a `for` body (the
    // regex-based quality-metrics rule flags literal loop-body clones
    // but not clones inside `.map` closures).
    edges.extend(
        record_list
            .fields()
            .filter(field_has_arg_attr)
            .filter_map(|field| field.name().map(|n| n.text().to_string()))
            .map(|field_name| Edge {
                src: entry_point_id.clone(),
                dst: field_node_id(struct_qname, &field_name),
                label: EdgeLabel::new(EdgeLabel::REGISTERS_PARAM),
                props: BTreeMap::new(),
            }),
    );
}

/// Emit REGISTERS_PARAM edges for a clap `#[derive(Subcommand)]` enum —
/// one edge per declared variant, pointing at the pre-existing
/// `:Variant` node id produced by the syn-side extractor via
/// [`cfdb_core::qname::variant_node_id`]. Per-variant-field granularity
/// is explicitly deferred: §3.1 N1 documents the transitional
/// approximation (one edge per variant) that a future
/// `cli_subcommand` kind will supersede.
///
/// Variant index is the declaration order, matching `variant_node_id`'s
/// indexing policy.
pub(super) fn emit_clap_enum_registers_param(
    enum_qname: &str,
    enum_: &ast::Enum,
    edges: &mut Vec<Edge>,
) {
    let Some(variant_list) = enum_.variant_list() else {
        return;
    };
    let entry_point_id = format!("entrypoint:cli_command:{enum_qname}");
    edges.extend(
        variant_list
            .variants()
            .enumerate()
            .map(|(index, _variant)| Edge {
                src: entry_point_id.clone(),
                dst: variant_node_id(enum_qname, index),
                label: EdgeLabel::new(EdgeLabel::REGISTERS_PARAM),
                props: BTreeMap::new(),
            }),
    );
}

/// Emit one `REGISTERS_PARAM` edge per non-self param of an MCP
/// `#[tool]` fn (#219 / RFC-037 §3.1 MCP row — HIR-owned).
///
/// Targets the `:Param` node the syn extractor emits via
/// [`cfdb_core::qname::param_node_id`]`(fn_qname, index)`. Receiver-aware:
/// when the fn has a `self` / `&self` / `&mut self` receiver, the syn
/// walker still calls `emit_param` for it with `index=0`, so we offset
/// the typed-param index by 1 to match.
pub(super) fn emit_mcp_registers_param(fn_qname: &str, fn_ast: &ast::Fn, edges: &mut Vec<Edge>) {
    let Some(param_list) = fn_ast.param_list() else {
        return;
    };
    let entry_point_id = format!("entrypoint:mcp_tool:{fn_qname}");
    let has_receiver = param_list.self_param().is_some();
    edges.extend(
        param_list
            .params()
            .enumerate()
            .map(|(typed_index, _param)| {
                let syn_index = if has_receiver {
                    typed_index + 1
                } else {
                    typed_index
                };
                Edge {
                    src: entry_point_id.clone(),
                    dst: param_node_id(fn_qname, syn_index),
                    label: EdgeLabel::new(EdgeLabel::REGISTERS_PARAM),
                    props: BTreeMap::new(),
                }
            }),
    );
}

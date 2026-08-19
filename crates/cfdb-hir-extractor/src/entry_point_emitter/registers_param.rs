use std::collections::BTreeMap;

use cfdb_core::fact::Edge;
use cfdb_core::qname::{entrypoint_node_id, field_node_id, param_node_id, variant_node_id};
use cfdb_core::schema::EdgeLabel;

use ra_ap_syntax::ast::{self, AstNode, HasAttrs, HasName};

pub(super) fn has_clap_derive<N: HasAttrs>(item: &N) -> bool {
    item.attrs().any(|attr| {
        let text = attr.syntax().to_string();
        if !text.contains("derive") {
            return false;
        }
        text.contains("Parser") || text.contains("Subcommand")
    })
}

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

pub(super) fn emit_clap_struct_registers_param(
    struct_identity: &str,
    strukt: &ast::Struct,
    edges: &mut Vec<Edge>,
) {
    let Some(ast::FieldList::RecordFieldList(record_list)) = strukt.field_list() else {
        return;
    };
    let entry_point_id = entrypoint_node_id("cli_command", struct_identity);
    edges.extend(
        record_list
            .fields()
            .filter(field_has_arg_attr)
            .filter_map(|field| field.name().map(|n| n.text().to_string()))
            .map(|field_name| Edge {
                src: entry_point_id.clone(),
                dst: field_node_id(struct_identity, &field_name),
                label: EdgeLabel::new(EdgeLabel::REGISTERS_PARAM),
                props: BTreeMap::new(),
            }),
    );
}

pub(super) fn emit_clap_enum_registers_param(
    enum_identity: &str,
    enum_: &ast::Enum,
    edges: &mut Vec<Edge>,
) {
    let Some(variant_list) = enum_.variant_list() else {
        return;
    };
    let entry_point_id = entrypoint_node_id("cli_command", enum_identity);
    edges.extend(
        variant_list
            .variants()
            .enumerate()
            .map(|(index, _variant)| Edge {
                src: entry_point_id.clone(),
                dst: variant_node_id(enum_identity, index),
                label: EdgeLabel::new(EdgeLabel::REGISTERS_PARAM),
                props: BTreeMap::new(),
            }),
    );
}

pub(super) fn emit_mcp_registers_param(fn_identity: &str, fn_ast: &ast::Fn, edges: &mut Vec<Edge>) {
    let Some(param_list) = fn_ast.param_list() else {
        return;
    };
    let entry_point_id = entrypoint_node_id("mcp_tool", fn_identity);
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
                    dst: param_node_id(fn_identity, syn_index),
                    label: EdgeLabel::new(EdgeLabel::REGISTERS_PARAM),
                    props: BTreeMap::new(),
                }
            }),
    );
}

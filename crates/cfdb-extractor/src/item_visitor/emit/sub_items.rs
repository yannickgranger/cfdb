use std::collections::BTreeMap;

use cfdb_core::fact::{Edge, Node, PropValue};
use cfdb_core::qname::{field_node_id, item_node_id, param_node_id, variant_node_id};
use cfdb_core::schema::{EdgeLabel, Label};

use crate::item_visitor::ItemVisitor;

impl ItemVisitor<'_> {
    #[allow(clippy::too_many_arguments)]
    pub(in crate::item_visitor) fn emit_param(
        &mut self,
        parent_qname: &str,
        index: usize,
        name: &str,
        is_self: bool,
        type_path: &str,
        type_normalized: &str,
        syn_type: Option<&syn::Type>,
    ) {
        let parent_identity = self.target.identity(parent_qname);
        let id = param_node_id(&parent_identity, index);
        let mut props = BTreeMap::new();
        props.insert("index".into(), PropValue::Int(index as i64));
        props.insert("is_self".into(), PropValue::Bool(is_self));
        props.insert("name".into(), PropValue::Str(name.to_string()));
        props.insert(
            "parent_qname".into(),
            PropValue::Str(parent_qname.to_string()),
        );
        props.insert(
            "type_normalized".into(),
            PropValue::Str(type_normalized.to_string()),
        );
        props.insert("type_path".into(), PropValue::Str(type_path.to_string()));
        self.emitter.emit_node(Node {
            id: id.clone(),
            label: Label::new(Label::PARAM),
            props,
        });
        if type_normalized != "?" {
            if let Some(ty) = syn_type {
                self.emitter.deferred_type_of.push((
                    id.clone(),
                    type_normalized.to_string(),
                    "Param",
                    ty.clone(),
                    self.target.clone(),
                ));
            }
        }
        self.emitter.emit_edge(Edge {
            src: item_node_id(&parent_identity),
            dst: id,
            label: EdgeLabel::new(EdgeLabel::HAS_PARAM),
            props: BTreeMap::new(),
        });
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::item_visitor) fn emit_field(
        &mut self,
        src_id: &str,
        parent_qname: &str,
        index: usize,
        name: &str,
        type_normalized: &str,
        type_path: &str,
        syn_type: &syn::Type,
    ) {
        let id = field_node_id(&self.target.identity(parent_qname), name);
        let mut props = BTreeMap::new();
        props.insert("index".into(), PropValue::Int(index as i64));
        props.insert("name".into(), PropValue::Str(name.to_string()));
        props.insert(
            "parent_qname".into(),
            PropValue::Str(parent_qname.to_string()),
        );
        props.insert(
            "type_normalized".into(),
            PropValue::Str(type_normalized.to_string()),
        );
        props.insert("type_path".into(), PropValue::Str(type_path.to_string()));
        self.emitter.emit_node(Node {
            id: id.clone(),
            label: Label::new(Label::FIELD),
            props,
        });
        if type_normalized != "?" {
            self.emitter.deferred_type_of.push((
                id.clone(),
                type_normalized.to_string(),
                "Field",
                syn_type.clone(),
                self.target.clone(),
            ));
        }
        self.emitter.emit_edge(Edge {
            src: src_id.to_string(),
            dst: id,
            label: EdgeLabel::new(EdgeLabel::HAS_FIELD),
            props: BTreeMap::new(),
        });
    }

    pub(in crate::item_visitor) fn emit_field_list(
        &mut self,
        src_id: &str,
        fields: &syn::Fields,
        parent_qname: &str,
    ) {
        match fields {
            syn::Fields::Named(named) => {
                for (index, f) in named.named.iter().enumerate() {
                    if let Some(ident) = &f.ident {
                        let field_name = ident.to_string();
                        let ty = crate::type_render::render_type_string(&f.ty);
                        self.emit_field(src_id, parent_qname, index, &field_name, &ty, &ty, &f.ty);
                    }
                }
            }
            syn::Fields::Unnamed(tuple) => {
                for (index, f) in tuple.unnamed.iter().enumerate() {
                    let field_name = format!("_{index}");
                    let ty = crate::type_render::render_type_string(&f.ty);
                    self.emit_field(src_id, parent_qname, index, &field_name, &ty, &ty, &f.ty);
                }
            }
            syn::Fields::Unit => {}
        }
    }

    pub(in crate::item_visitor) fn emit_variant(
        &mut self,
        enum_qname: &str,
        index: usize,
        name: &str,
        payload_kind: &str,
    ) -> (String, String) {
        let enum_identity = self.target.identity(enum_qname);
        let id = variant_node_id(&enum_identity, index);
        let variant_qname = format!("{enum_qname}::{name}");
        let mut props = BTreeMap::new();
        props.insert("index".into(), PropValue::Int(index as i64));
        props.insert("name".into(), PropValue::Str(name.to_string()));
        props.insert(
            "parent_qname".into(),
            PropValue::Str(enum_qname.to_string()),
        );
        props.insert(
            "payload_kind".into(),
            PropValue::Str(payload_kind.to_string()),
        );
        self.emitter.emit_node(Node {
            id: id.clone(),
            label: Label::new(Label::VARIANT),
            props,
        });
        self.emitter.emit_edge(Edge {
            src: item_node_id(&enum_identity),
            dst: id.clone(),
            label: EdgeLabel::new(EdgeLabel::HAS_VARIANT),
            props: BTreeMap::new(),
        });
        (id, variant_qname)
    }

    pub(in crate::item_visitor) fn emit_const_table(
        &mut self,
        table: crate::const_table::RecognizedConstTable,
        parent_item_id: &str,
    ) {
        let crate::const_table::RecognizedConstTable {
            qname,
            name,
            crate_name,
            module_qpath,
            element_type,
            entries,
            is_test,
        } = table;
        let id = format!("const_table:{qname}");
        let mut props = BTreeMap::new();
        props.insert("crate".into(), PropValue::Str(crate_name));
        props.insert(
            "element_type".into(),
            PropValue::Str(element_type.as_wire_str().to_string()),
        );
        props.insert(
            "entries_hash".into(),
            PropValue::Str(crate::const_table::entries_hash_hex(&entries)),
        );
        props.insert(
            "entries_normalized".into(),
            PropValue::Str(crate::const_table::entries_normalized_json(&entries)),
        );
        props.insert(
            "entries_sample".into(),
            PropValue::Str(crate::const_table::entries_sample_json(&entries)),
        );
        props.insert("entry_count".into(), PropValue::Int(entries.len() as i64));
        props.insert("is_test".into(), PropValue::Bool(is_test));
        props.insert("module_qpath".into(), PropValue::Str(module_qpath));
        props.insert("name".into(), PropValue::Str(name));
        props.insert("qname".into(), PropValue::Str(qname));
        self.emitter.emit_node(Node {
            id: id.clone(),
            label: Label::new(Label::CONST_TABLE),
            props,
        });
        self.emitter.emit_edge(Edge {
            src: parent_item_id.to_string(),
            dst: id,
            label: EdgeLabel::new(EdgeLabel::HAS_CONST_TABLE),
            props: BTreeMap::new(),
        });
    }
}

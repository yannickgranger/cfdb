//! Sub-item emitters on [`ItemVisitor`] — the satellite-node methods that
//! live below an `:Item`: `:Param`, `:Field`, `:Variant`, `:ConstTable`.
//!
//! Split out of [`super`] (#350) to keep each file under the 500-LOC
//! budget. Both sibling modules share the same `impl ItemVisitor<'_>`
//! block — Rust allows multiple inherent impls on the same type, so the
//! split is purely source-level. Public surface is unchanged.

use std::collections::BTreeMap;

use cfdb_core::fact::{Edge, Node, PropValue};
use cfdb_core::qname::{field_node_id, item_node_id, param_node_id, variant_node_id};
use cfdb_core::schema::{EdgeLabel, Label};

use crate::item_visitor::ItemVisitor;

impl ItemVisitor<'_> {
    /// Emit one `:Param` node + `HAS_PARAM` edge for a fn/method
    /// parameter (#209, RFC-036 §3.1). Canonical id formula lives in
    /// `cfdb-core::qname::param_node_id`; every extractor (syn-based
    /// today, HIR-based tomorrow) routes through it so
    /// `REGISTERS_PARAM` edges emitted by the HIR side land on the
    /// same `:Param` node ids these emit.
    #[allow(clippy::too_many_arguments)] // #239: syn_type carries original type for render_type_inner fallback
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
        let id = param_node_id(parent_qname, index);
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
        // Queue TYPE_OF resolution (RFC-037 §3.4, #220; #239). Skip
        // trivial renderings (`"?"`) and `self`-receiver params
        // (`syn_type` is `None` for receivers — they carry `Self`,
        // never an `:Item` in the workspace). The source node id
        // (`id`) is captured now because by the time the post-walk
        // pass runs, `parent_qname` + `index` alone are not enough
        // to reconstruct it without re-deriving the formula. The
        // stored `syn::Type` powers the wrapper-unwrap third tier
        // in `resolve_deferred_type_of`.
        if type_normalized != "?" {
            if let Some(ty) = syn_type {
                self.emitter.deferred_type_of.push((
                    id.clone(),
                    type_normalized.to_string(),
                    "Param",
                    ty.clone(),
                ));
            }
        }
        self.emitter.emit_edge(Edge {
            src: item_node_id(parent_qname),
            dst: id,
            label: EdgeLabel::new(EdgeLabel::HAS_PARAM),
            props: BTreeMap::new(),
        });
    }

    /// Emit a single `:Field` node + `HAS_FIELD` edge.
    ///
    /// `src_id` is the node id of the owner — `item_node_id(struct_qname)`
    /// for struct fields, `variant_node_id(enum_qname, i)` for enum-variant
    /// fields (#218, RFC-037 §3.3). Previously hardcoded to
    /// `item_node_id(parent_qname)`, which only worked for structs.
    #[allow(clippy::too_many_arguments)] // #239: syn_type carries original type for render_type_inner fallback
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
        let id = field_node_id(parent_qname, name);
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
        // Queue TYPE_OF resolution (RFC-037 §3.4, #220; #239). Skip
        // trivial renderings (`"?"`) that definitely won't resolve.
        // The source node id (`id`) is the `:Field` node id, not the
        // owning struct/variant — TYPE_OF edges flow Field → Item.
        // The stored `syn::Type` powers the wrapper-unwrap third tier
        // in `resolve_deferred_type_of`.
        if type_normalized != "?" {
            self.emitter.deferred_type_of.push((
                id.clone(),
                type_normalized.to_string(),
                "Field",
                syn_type.clone(),
            ));
        }
        self.emitter.emit_edge(Edge {
            src: src_id.to_string(),
            dst: id,
            label: EdgeLabel::new(EdgeLabel::HAS_FIELD),
            props: BTreeMap::new(),
        });
    }

    /// Walk a `syn::Fields` (named, tuple, or unit) and emit one `:Field`
    /// per element. Shared between `visit_item_struct` (struct body),
    /// `visit_item_enum` (per-variant record and tuple payloads), and
    /// any future variant of the same pattern.
    ///
    /// `src_id` is passed to `emit_field` as the HAS_FIELD edge source:
    /// the struct's `:Item` node id, or the variant's `:Variant` node id.
    /// `parent_qname` becomes the field's `parent_qname` prop (e.g.
    /// `crate::Foo` for struct fields, `crate::Bar::Variant` for variant
    /// fields).
    ///
    /// Tuple elements (named or unnamed) use synthetic names `_0`, `_1`, ...
    /// matching the `:Field.name` descriptor convention.
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

    /// Emit one `:Variant` node + `HAS_VARIANT` edge for an enum variant
    /// (#218, RFC-037 §3.3). Canonical id formula lives in
    /// `cfdb-core::qname::variant_node_id`; the caller is responsible for
    /// walking variant payload fields separately via `emit_field_list`.
    ///
    /// `payload_kind` is one of `"unit" | "tuple" | "struct"` — derived
    /// from the variant's `syn::Fields` by the caller.
    ///
    /// Returns `(variant_id, variant_qname)` — the node id for use as the
    /// `HAS_FIELD` edge src on variant fields, and the qname
    /// (`Enum::Variant`) for use as the field's `parent_qname` prop.
    pub(in crate::item_visitor) fn emit_variant(
        &mut self,
        enum_qname: &str,
        index: usize,
        name: &str,
        payload_kind: &str,
    ) -> (String, String) {
        let id = variant_node_id(enum_qname, index);
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
            src: item_node_id(enum_qname),
            dst: id.clone(),
            label: EdgeLabel::new(EdgeLabel::HAS_VARIANT),
            props: BTreeMap::new(),
        });
        (id, variant_qname)
    }

    /// Emit one `:ConstTable` node and the `(:Item) -[:HAS_CONST_TABLE]->
    /// (:ConstTable)` edge from a recognized const-table candidate
    /// ([`crate::const_table::recognize_const_table`]). RFC-040 §3.1 / §3.2.
    ///
    /// `parent_item_id` is the `:Item` node id returned by `emit_item` for
    /// the parent const — the edge flows parent → satellite, matching the
    /// rest of the `HAS_*` family. The id namespace is disjoint from
    /// `:Item` (`const_table:{qname}` vs `item:{qname}`).
    ///
    /// The `element_type` wire string is constructed exclusively via
    /// [`crate::const_table::ElementType::as_wire_str`] — the single owner
    /// of the closed-set vocabulary `{"str", "u32", "i32", "u64", "i64"}`
    /// per the RFC-038 §3.1 invariant-owner pattern (R2 solid-architect B2).
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

//! Inherent helper + `emit_*` methods on [`ItemVisitor`]. Split out of
//! `item_visitor.rs` to keep each file under the 500-LOC budget (#128).
//! These methods are only reachable from the `syn::Visit` impl (see
//! sibling `visits` module) and from each other.

use std::collections::BTreeMap;

use cfdb_core::fact::{Edge, Node, PropValue};
use cfdb_core::qname::{
    field_node_id, item_node_id, item_qname, method_qname, module_qpath, param_node_id,
    variant_node_id,
};
use cfdb_core::schema::{EdgeLabel, Label};

use crate::attrs::{attrs_contain_hash_test, extract_cfg_feature_gate, extract_deprecated_attr};
use crate::Emitter;

use super::{
    impl_block_name, impl_block_qname, parse_syn_visibility, resolve_target_qname, ItemVisitor,
};

/// Emit a `:CallSite` node + `INVOKES_AT` edge from the owning `:Item`.
///
/// Centralises the prop shape, node label, and edge wiring shared by the
/// two CallSite emission paths in this crate (audit 2026-W17 / EPIC #273
/// / Pattern 3 fan-out):
///
/// - **Body-call sites** ([`crate::call_visitor::CallSiteVisitor::emit_call_site`]) —
///   call expressions inside a fn body. The caller computes
///   `cs_id = format!("callsite:{caller_qname}:{callee_path}:{local_idx}")`.
/// - **Attribute-ref sites**
///   ([`ItemVisitor::emit_attr_call_site`]) — name references inside
///   attributes such as `#[serde(default = "Utc::now")]`. The caller
///   computes `cs_id = format!("callsite:{parent_qname}.{field_name}:{callee_path}:0")`
///   and passes the `field` prop via `extra_props`.
///
/// The id format and the attr-only `field` prop stay caller-side; this
/// helper owns the prop shape (`callee_last_segment` derivation, the
/// `resolver="syn"` discriminator, the `callee_resolved=false` default),
/// the `Label::CALL_SITE` node emission, and the `INVOKES_AT` edge from
/// `item_node_id(caller_qname)`.
///
/// `extra_props` is merged after the canonical props so the attr-ref
/// path can attach `field=<field_name>`. Body-call sites pass an empty
/// map.
#[allow(clippy::too_many_arguments)] // 9 args — :CallSite shape is wide; cs_id + extra_props stay caller-side because the two emission paths differ on id format and on whether they attach a `field` prop (audit 2026-W17 / EPIC #273 / Pattern 3 fan-out). `line` is the real source-line number from the call expression's syn span (#273 / F-005); 0 = unknown / synthetic.
pub(crate) fn emit_call_site_node_and_edge(
    emitter: &mut Emitter,
    cs_id: String,
    caller_qname: &str,
    callee_path: &str,
    kind: &str,
    file: String,
    line: usize,
    is_test: bool,
    extra_props: BTreeMap<String, PropValue>,
) {
    let last_segment = callee_path
        .rsplit("::")
        .next()
        .unwrap_or(callee_path)
        .to_string();

    let mut props = BTreeMap::new();
    props.insert(
        "caller_qname".into(),
        PropValue::Str(caller_qname.to_string()),
    );
    props.insert(
        "callee_path".into(),
        PropValue::Str(callee_path.to_string()),
    );
    props.insert("callee_last_segment".into(), PropValue::Str(last_segment));
    props.insert("kind".into(), PropValue::Str(kind.to_string()));
    props.insert("file".into(), PropValue::Str(file));
    props.insert("line".into(), PropValue::Int(line as i64));
    props.insert("is_test".into(), PropValue::Bool(is_test));
    // SchemaVersion v0.1.3+ discriminator (Label::CALL_SITE doc, #83).
    props.insert("resolver".into(), PropValue::Str("syn".to_string()));
    props.insert("callee_resolved".into(), PropValue::Bool(false));
    props.extend(extra_props);

    emitter.emit_node(Node {
        id: cs_id.clone(),
        label: Label::new(Label::CALL_SITE),
        props,
    });
    emitter.emit_edge(Edge {
        src: item_node_id(caller_qname),
        dst: cs_id,
        label: EdgeLabel::new(EdgeLabel::INVOKES_AT),
        props: BTreeMap::new(),
    });
}

/// Insert `cfg_gate` (when present) + `is_deprecated` + `deprecation_since`
/// (when present) into the prop map. Centralises the shape used by every
/// `:Item` emit path so a future schema change touches one site
/// (audit 2026-W17 / EPIC #273 / Pattern 3 fan-out).
fn insert_attr_metadata_props(props: &mut BTreeMap<String, PropValue>, attrs: &[syn::Attribute]) {
    if let Some(gate) = extract_cfg_feature_gate(attrs) {
        props.insert("cfg_gate".into(), PropValue::Str(gate.to_string()));
    }
    // Deprecation facts (#106 / RFC addendum §A2.2 row 3) —
    // extractor-time per DDD + rust-systems verdicts. `is_deprecated`
    // always emitted (false by default so downstream classifier
    // queries can treat absence as a data gap vs. false). `deprecation_since`
    // only emitted when the `#[deprecated(since = "X")]` form is used.
    let (is_deprecated, deprecation_since) = extract_deprecated_attr(attrs);
    props.insert("is_deprecated".into(), PropValue::Bool(is_deprecated));
    if let Some(since) = deprecation_since {
        props.insert("deprecation_since".into(), PropValue::Str(since));
    }
}

impl ItemVisitor<'_> {
    pub(super) fn current_module_qpath(&self) -> String {
        module_qpath(&self.module_stack)
    }

    pub(super) fn qname(&self, item_name: &str) -> String {
        item_qname(&self.module_stack, item_name)
    }

    /// Emit `src_id -[IN_MODULE]-> module:<current_module_qpath>` when an
    /// enclosing `:Module` node exists.
    ///
    /// The schema declares `IN_MODULE` from `[Item, File]` to `[Module]`
    /// (`cfdb-core/src/schema/describe/edges.rs`), but the extractor used
    /// to emit only the `IN_CRATE` edge — `SchemaDescribe()` lied to
    /// consumers and any Cypher walking `Item -[:IN_MODULE]-> Module`
    /// returned zero rows (#267, audit ID CFDB-EXT-H1).
    ///
    /// `:Module` nodes are emitted only by `visit_item_mod` for nested
    /// `mod` declarations — the crate root has no `:Module` node. The
    /// `module_stack` invariant (see `ItemVisitor::module_stack` doc) is
    /// that element 0 is always the crate name, so an item is at the
    /// crate root iff `module_stack.len() == 1`. In that case there is
    /// no enclosing `:Module` to point at and this method is a no-op —
    /// the existing `IN_CRATE` edge already routes the item to its
    /// `:Crate` node.
    pub(super) fn emit_in_module_edge(&mut self, src_id: &str) {
        if self.module_stack.len() <= 1 {
            return;
        }
        let qpath = self.current_module_qpath();
        let module_id = format!("module:{qpath}");
        self.emitter.emit_edge(Edge {
            src: src_id.to_string(),
            dst: module_id,
            label: EdgeLabel::new(EdgeLabel::IN_MODULE),
            props: BTreeMap::new(),
        });
    }

    pub(super) fn is_in_test_mod(&self) -> bool {
        self.test_mod_depth > 0
    }

    /// `is_test` for a fn item: true when either (a) the enclosing module is
    /// `#[cfg(test)]`-gated, or (b) the fn itself carries a bare `#[test]`
    /// attribute. This is the single OR site — non-fn items stay on the
    /// module-depth signal alone (struct/enum/etc. have no libtest-native
    /// marker). Council-cfdb-wiring §B.1.1.
    pub(super) fn fn_is_test(&self, attrs: &[syn::Attribute]) -> bool {
        self.is_in_test_mod() || attrs_contain_hash_test(attrs)
    }

    pub(super) fn emit_item(
        &mut self,
        name: &str,
        kind: &str,
        line: usize,
        vis: &syn::Visibility,
        attrs: &[syn::Attribute],
    ) -> String {
        self.emit_item_with_flags(
            name,
            kind,
            line,
            self.is_in_test_mod(),
            vis,
            attrs,
            None,
            None,
        )
    }

    /// Like [`emit_item`] but the caller supplies the `is_test` flag
    /// explicitly. Used by the fn-item visit path so a bare `#[test]` fn
    /// outside a `#[cfg(test)]` module is still tagged `is_test=true`.
    ///
    /// `signature` is the canonical fn signature string
    /// (`fn(i32) -> bool`) produced by
    /// [`crate::type_render::render_fn_signature`]. Pass `Some(sig)` on
    /// fn / method kinds so `:Item.signature` lands in the graph —
    /// required by the `signature_divergent` UDF (#47). Non-fn kinds
    /// (struct, enum, trait, const, …) pass `None` and the prop is
    /// omitted.
    ///
    /// `impl_target` routes the impl-method emission path through this
    /// helper instead of duplicating ~95 lines of `:Item` prop
    /// construction (audit 2026-W17 / EPIC #273 / Pattern 3 F-002).
    /// When `Some(target)` the qname is computed via
    /// [`cfdb_core::qname::method_qname`] (`module::Target::method`)
    /// instead of the free-item formula
    /// (`module::name`), and the `impl_target` prop is emitted on the
    /// resulting `:Item` node. When `None` the free-item formula is
    /// used and no `impl_target` prop is emitted. This is the single
    /// owner of `:Item` prop emission across free items, impl blocks,
    /// and impl methods.
    #[allow(clippy::too_many_arguments)] // 9 args — fn/method :Item shape is wide; impl_target is the impl-method routing knob (audit F-002), the rest are name/kind/line/is_test/vis/attrs/signature
    pub(super) fn emit_item_with_flags(
        &mut self,
        name: &str,
        kind: &str,
        line: usize,
        is_test: bool,
        vis: &syn::Visibility,
        attrs: &[syn::Attribute],
        signature: Option<&str>,
        impl_target: Option<&str>,
    ) -> String {
        let qname = match impl_target {
            Some(target) => method_qname(&self.module_stack, target, name),
            None => self.qname(name),
        };
        let id = item_node_id(&qname);
        let mut props = BTreeMap::new();
        props.insert("qname".into(), PropValue::Str(qname.clone()));
        props.insert("name".into(), PropValue::Str(name.to_string()));
        props.insert("kind".into(), PropValue::Str(kind.to_string()));
        props.insert("crate".into(), PropValue::Str(self.crate_name.clone()));
        props.insert(
            "bounded_context".into(),
            PropValue::Str(self.bounded_context.clone()),
        );
        props.insert(
            "module_qpath".into(),
            PropValue::Str(self.current_module_qpath()),
        );
        // `impl_target` prop is emitted only on the impl-method path so
        // queries can distinguish methods from free fns by the prop's
        // presence (audit F-002 routing). Free items don't carry an
        // impl target — the prop is absent, not the empty string.
        if let Some(target) = impl_target {
            props.insert("impl_target".into(), PropValue::Str(target.to_string()));
        }
        props.insert("file".into(), PropValue::Str(self.file_path.clone()));
        props.insert("line".into(), PropValue::Int(line as i64));
        props.insert("is_test".into(), PropValue::Bool(is_test));
        props.insert(
            "visibility".into(),
            PropValue::Str(parse_syn_visibility(vis).to_string()),
        );
        insert_attr_metadata_props(&mut props, attrs);
        // `:Item.signature` — canonical fn signature string (#47). Only
        // emitted on fn / method kinds. Non-fn kinds pass `None` and the
        // prop is absent, which queries can distinguish from the empty
        // string via `IS NULL`.
        if let Some(sig) = signature {
            props.insert("signature".into(), PropValue::Str(sig.to_string()));
        }
        self.emitter.emit_node(Node {
            id: id.clone(),
            label: Label::new(Label::ITEM),
            props,
        });
        // RETURNS / TYPE_OF post-walk resolution requires the set of
        // every emitted `:Item` qname (RFC-037 §3.2, #216). The set
        // lives on the workspace-scoped `Emitter` so the resolution
        // pass in `extract_workspace` sees items across every file.
        self.emitter.emitted_item_qnames.insert(qname.clone());
        self.emitter.emit_edge(Edge {
            src: id.clone(),
            dst: self.crate_id.clone(),
            label: EdgeLabel::new(EdgeLabel::IN_CRATE),
            props: BTreeMap::new(),
        });
        // IN_MODULE membership for the deepest enclosing module (#267).
        // No-op at crate root where no `:Module` node exists.
        self.emit_in_module_edge(&id);
        id
    }

    /// Emit an `:Item { kind: "impl_block" }` node for the current `impl`
    /// block plus its `IMPLEMENTS` + `IMPLEMENTS_FOR` edges (#42 / RFC
    /// Study 002 §11.4b).
    ///
    /// The impl-block's qname encodes the module path, the normalised
    /// target type, and (when present) the trait path, so two trait
    /// impls targeting the same type land on distinct nodes:
    ///
    /// ```text
    /// impl Foo { ... }            → <module>::Foo::impl
    /// impl Bar for Foo { ... }    → <module>::Foo::impl_Bar
    /// impl Baz for Foo { ... }    → <module>::Foo::impl_Baz
    /// ```
    ///
    /// Trait paths containing `::` are flattened to `_` for use in the
    /// qname segment — the original trait path is preserved in the
    /// `IMPLEMENTS` edge target so queries can resolve back to the
    /// canonical trait node.
    pub(super) fn emit_impl_block(
        &mut self,
        target: &str,
        trait_qname: Option<&str>,
        line: usize,
        attrs: &[syn::Attribute],
    ) {
        let impl_qname = impl_block_qname(&self.module_stack, target, trait_qname);
        let impl_id = item_node_id(&impl_qname);

        let mut props = BTreeMap::new();
        props.insert("qname".into(), PropValue::Str(impl_qname.clone()));
        props.insert(
            "name".into(),
            PropValue::Str(impl_block_name(target, trait_qname)),
        );
        props.insert("kind".into(), PropValue::Str("impl_block".into()));
        props.insert("crate".into(), PropValue::Str(self.crate_name.clone()));
        props.insert(
            "bounded_context".into(),
            PropValue::Str(self.bounded_context.clone()),
        );
        props.insert(
            "module_qpath".into(),
            PropValue::Str(self.current_module_qpath()),
        );
        props.insert("file".into(), PropValue::Str(self.file_path.clone()));
        // Real source line of the `impl` token — feeds line-precision
        // queries the same way fn / struct / enum lines do (#273 /
        // F-005). 0 for synthetic / macro-expanded impls.
        props.insert("line".into(), PropValue::Int(line as i64));
        props.insert("is_test".into(), PropValue::Bool(self.is_in_test_mod()));
        // impl blocks carry no visibility modifier of their own in Rust;
        // the impl's effective reachability is the intersection of the
        // target type's and the trait's visibilities — treat the impl
        // block itself as private for the cfdb vocabulary (council
        // wiring §B.1.1 default).
        props.insert("visibility".into(), PropValue::Str("private".into()));
        props.insert("impl_target".into(), PropValue::Str(target.into()));
        if let Some(t) = trait_qname {
            props.insert("impl_trait".into(), PropValue::Str(t.into()));
        }
        insert_attr_metadata_props(&mut props, attrs);

        self.emitter.emit_node(Node {
            id: impl_id.clone(),
            label: Label::new(Label::ITEM),
            props,
        });
        // Track impl-block qname for RETURNS / TYPE_OF post-walk
        // resolution (RFC-037 §3.2, #216). Impl-block qnames don't
        // typically appear as return types, but we populate the set
        // consistently for every emitted `:Item` so any future fact
        // type that resolves on `:Item` qnames is accurate.
        self.emitter.emitted_item_qnames.insert(impl_qname.clone());
        self.emitter.emit_edge(Edge {
            src: impl_id.clone(),
            dst: self.crate_id.clone(),
            label: EdgeLabel::new(EdgeLabel::IN_CRATE),
            props: BTreeMap::new(),
        });
        // IN_MODULE membership for the deepest enclosing module (#267).
        // No-op at crate root where no `:Module` node exists.
        self.emit_in_module_edge(&impl_id);

        // IMPLEMENTS_FOR — always emitted. Target resolution via the
        // `item:<qname>` id formula. The dst may dangle when the target
        // type is defined outside the workspace; the petgraph ingest
        // layer emits a non-fatal warning rather than failing.
        let target_qname = resolve_target_qname(&self.module_stack, target);
        self.emitter.emit_edge(Edge {
            src: impl_id.clone(),
            dst: item_node_id(&target_qname),
            label: EdgeLabel::new(EdgeLabel::IMPLEMENTS_FOR),
            props: BTreeMap::new(),
        });

        // IMPLEMENTS — trait impls only.
        if let Some(t) = trait_qname {
            let trait_resolved = resolve_target_qname(&self.module_stack, t);
            self.emitter.emit_edge(Edge {
                src: impl_id,
                dst: item_node_id(&trait_resolved),
                label: EdgeLabel::new(EdgeLabel::IMPLEMENTS),
                props: BTreeMap::new(),
            });
        }
    }

    /// Emit a CallSite for an attribute-based name-reference to a callable
    /// (e.g. `#[serde(default = "Utc::now")]`). The owning `Item` is the
    /// struct that holds the field, so the INVOKES_AT edge flows from the
    /// struct to the CallSite — same shape the query evaluator uses to
    /// surface ban-rule hits for fn-body call sites.
    ///
    /// The CallSite id encodes the field name so two fields on the same
    /// struct with the same callee path produce distinct nodes (G1
    /// determinism requirement).
    pub(super) fn emit_attr_call_site(
        &mut self,
        parent_qname: &str,
        field_name: &str,
        callee_path: &str,
        kind: &str,
        line: usize,
    ) {
        let cs_id = format!("callsite:{parent_qname}.{field_name}:{callee_path}:0");
        let mut extra = BTreeMap::new();
        extra.insert("field".into(), PropValue::Str(field_name.to_string()));
        emit_call_site_node_and_edge(
            self.emitter,
            cs_id,
            parent_qname,
            callee_path,
            kind,
            self.file_path.clone(),
            line,
            self.is_in_test_mod(),
            extra,
        );
    }

    /// Emit one `:Param` node + `HAS_PARAM` edge for a fn/method
    /// parameter (#209, RFC-036 §3.1). Canonical id formula lives in
    /// `cfdb-core::qname::param_node_id`; every extractor (syn-based
    /// today, HIR-based tomorrow) routes through it so
    /// `REGISTERS_PARAM` edges emitted by the HIR side land on the
    /// same `:Param` node ids these emit.
    #[allow(clippy::too_many_arguments)] // #239: syn_type carries original type for render_type_inner fallback
    pub(super) fn emit_param(
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
    pub(super) fn emit_field(
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
    pub(super) fn emit_field_list(
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
    pub(super) fn emit_variant(
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
    pub(super) fn emit_const_table(
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

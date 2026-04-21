//! Inherent helper + `emit_*` methods on [`ItemVisitor`]. Split out of
//! `item_visitor.rs` to keep each file under the 500-LOC budget (#128).
//! These methods are only reachable from the `syn::Visit` impl (see
//! sibling `visits` module) and from each other.

use std::collections::BTreeMap;

use cfdb_core::fact::{Edge, Node, PropValue};
use cfdb_core::qname::{item_node_id, item_qname, module_qpath};
use cfdb_core::schema::{EdgeLabel, Label};

use crate::attrs::{
    attrs_contain_hash_test, extract_cfg_feature_gate, extract_deprecated_attr,
};

use super::{
    impl_block_name, impl_block_qname, parse_syn_visibility, resolve_target_qname, ItemVisitor,
};

impl ItemVisitor<'_> {
    pub(super) fn current_module_qpath(&self) -> String {
        module_qpath(&self.module_stack)
    }

    pub(super) fn qname(&self, item_name: &str) -> String {
        item_qname(&self.module_stack, item_name)
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
        self.emit_item_with_flags(name, kind, line, self.is_in_test_mod(), vis, attrs, None)
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
    #[allow(clippy::too_many_arguments)] // 8 args — fn/method :Item shape is wide (name/kind/line/is_test/vis/attrs/signature); a struct would add boilerplate without reducing cognitive load
    pub(super) fn emit_item_with_flags(
        &mut self,
        name: &str,
        kind: &str,
        line: usize,
        is_test: bool,
        vis: &syn::Visibility,
        attrs: &[syn::Attribute],
        signature: Option<&str>,
    ) -> String {
        let qname = self.qname(name);
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
        props.insert("file".into(), PropValue::Str(self.file_path.clone()));
        props.insert("line".into(), PropValue::Int(line as i64));
        props.insert("is_test".into(), PropValue::Bool(is_test));
        props.insert(
            "visibility".into(),
            PropValue::Str(parse_syn_visibility(vis).to_string()),
        );
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
        self.emitter.emit_edge(Edge {
            src: id.clone(),
            dst: self.crate_id.clone(),
            label: EdgeLabel::new(EdgeLabel::IN_CRATE),
            props: BTreeMap::new(),
        });
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
        props.insert("line".into(), PropValue::Int(0));
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
        if let Some(gate) = extract_cfg_feature_gate(attrs) {
            props.insert("cfg_gate".into(), PropValue::Str(gate.to_string()));
        }
        let (is_deprecated, deprecation_since) = extract_deprecated_attr(attrs);
        props.insert("is_deprecated".into(), PropValue::Bool(is_deprecated));
        if let Some(since) = deprecation_since {
            props.insert("deprecation_since".into(), PropValue::Str(since));
        }

        self.emitter.emit_node(Node {
            id: impl_id.clone(),
            label: Label::new(Label::ITEM),
            props,
        });
        self.emitter.emit_edge(Edge {
            src: impl_id.clone(),
            dst: self.crate_id.clone(),
            label: EdgeLabel::new(EdgeLabel::IN_CRATE),
            props: BTreeMap::new(),
        });

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
    ) {
        let cs_id = format!("callsite:{parent_qname}.{field_name}:{callee_path}:0");
        let last_segment = callee_path
            .rsplit("::")
            .next()
            .unwrap_or(callee_path)
            .to_string();
        let mut props = BTreeMap::new();
        props.insert(
            "caller_qname".into(),
            PropValue::Str(parent_qname.to_string()),
        );
        props.insert(
            "callee_path".into(),
            PropValue::Str(callee_path.to_string()),
        );
        props.insert("callee_last_segment".into(), PropValue::Str(last_segment));
        props.insert("kind".into(), PropValue::Str(kind.to_string()));
        props.insert("file".into(), PropValue::Str(self.file_path.clone()));
        props.insert("line".into(), PropValue::Int(0));
        props.insert("is_test".into(), PropValue::Bool(self.is_in_test_mod()));
        props.insert("field".into(), PropValue::Str(field_name.to_string()));
        // SchemaVersion v0.1.3+ discriminator (Label::CALL_SITE doc, #83).
        props.insert("resolver".into(), PropValue::Str("syn".to_string()));
        props.insert("callee_resolved".into(), PropValue::Bool(false));
        self.emitter.emit_node(Node {
            id: cs_id.clone(),
            label: Label::new(Label::CALL_SITE),
            props,
        });
        self.emitter.emit_edge(Edge {
            src: item_node_id(parent_qname),
            dst: cs_id,
            label: EdgeLabel::new(EdgeLabel::INVOKES_AT),
            props: BTreeMap::new(),
        });
    }

    pub(super) fn emit_field(&mut self, parent_qname: &str, name: &str, ty: &str) {
        let id = format!("field:{parent_qname}.{name}");
        let mut props = BTreeMap::new();
        props.insert("name".into(), PropValue::Str(name.to_string()));
        props.insert(
            "parent_qname".into(),
            PropValue::Str(parent_qname.to_string()),
        );
        props.insert("type_qname".into(), PropValue::Str(ty.to_string()));
        self.emitter.emit_node(Node {
            id: id.clone(),
            label: Label::new(Label::FIELD),
            props,
        });
        self.emitter.emit_edge(Edge {
            src: item_node_id(parent_qname),
            dst: id,
            label: EdgeLabel::new(EdgeLabel::HAS_FIELD),
            props: BTreeMap::new(),
        });
    }
}

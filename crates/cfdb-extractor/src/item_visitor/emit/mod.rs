use std::collections::BTreeMap;

use cfdb_core::fact::{build_item_props_common, Edge, Node, PropValue};
use cfdb_core::qname::{
    item_node_id, item_node_id_for_target, item_qname, method_qname, module_qpath,
};
use cfdb_core::schema::{EdgeLabel, Label};

use crate::attrs::{attrs_contain_hash_test, extract_cfg_feature_gate, extract_deprecated_attr};
use crate::Emitter;

use super::{
    impl_block_name, impl_block_qname, parse_syn_visibility, resolve_target_qname, ItemVisitor,
};

mod sub_items;

#[allow(clippy::too_many_arguments)]
pub(crate) fn emit_call_site_node_and_edge(
    emitter: &mut Emitter,
    cs_id: String,
    caller_qname: &str,
    caller_target: &cfdb_core::qname::TargetDiscriminator,
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
    props.insert("resolver".into(), PropValue::Str("syn".to_string()));
    props.insert("callee_resolved".into(), PropValue::Bool(false));
    props.extend(extra_props);

    emitter.emit_node(Node {
        id: cs_id.clone(),
        label: Label::new(Label::CALL_SITE),
        props,
    });
    emitter.emit_edge(Edge {
        src: item_node_id_for_target(caller_qname, caller_target),
        dst: cs_id,
        label: EdgeLabel::new(EdgeLabel::INVOKES_AT),
        props: BTreeMap::new(),
    });
}

pub(super) fn insert_attr_metadata_props(
    props: &mut BTreeMap<String, PropValue>,
    attrs: &[syn::Attribute],
) {
    if let Some(gate) = extract_cfg_feature_gate(attrs) {
        props.insert("cfg_gate".into(), PropValue::Str(gate.to_string()));
    }
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
    ) -> (String, String) {
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

    #[allow(clippy::too_many_arguments)]
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
    ) -> (String, String) {
        let qname = match impl_target {
            Some(target) => method_qname(&self.module_stack, target, name),
            None => self.qname(name),
        };
        let id = item_node_id_for_target(&qname, &self.target);
        let mut props = build_item_props_common(&qname, name, kind, &self.crate_name);
        props.insert(
            "bounded_context".into(),
            PropValue::Str(self.bounded_context.clone()),
        );
        props.insert(
            "module_qpath".into(),
            PropValue::Str(self.current_module_qpath()),
        );
        if let Some(target) = impl_target {
            props.insert("impl_target".into(), PropValue::Str(target.to_string()));
        }
        props.insert("file".into(), PropValue::Str(self.file_path.clone()));
        props.insert("line".into(), PropValue::Int(line as i64));
        props.insert("is_test".into(), PropValue::Bool(is_test));
        props.insert(
            "target".into(),
            PropValue::Str(self.target.as_wire_str().into_owned()),
        );
        props.insert(
            "visibility".into(),
            PropValue::Str(parse_syn_visibility(vis).to_string()),
        );
        insert_attr_metadata_props(&mut props, attrs);
        if let Some(sig) = signature {
            props.insert("signature".into(), PropValue::Str(sig.to_string()));
        }
        self.emitter.emit_node(Node {
            id: id.clone(),
            label: Label::new(Label::ITEM),
            props,
        });
        self.emitter.claim_item_qname(&qname, &self.target);
        if kind == "enum" {
            self.emitter.claim_enum_qname(&qname, &self.target);
        }
        self.emitter.emit_edge(Edge {
            src: id.clone(),
            dst: self.crate_id.clone(),
            label: EdgeLabel::new(EdgeLabel::IN_CRATE),
            props: BTreeMap::new(),
        });
        self.emit_in_module_edge(&id);
        (id, qname)
    }

    pub(super) fn emit_impl_block(
        &mut self,
        target: &str,
        trait_qname: Option<&str>,
        line: usize,
        attrs: &[syn::Attribute],
    ) {
        let impl_qname = impl_block_qname(&self.module_stack, target, trait_qname);
        let impl_id = item_node_id_for_target(&impl_qname, &self.target);

        let mut props = build_item_props_common(
            &impl_qname,
            &impl_block_name(target, trait_qname),
            "impl_block",
            &self.crate_name,
        );
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
        props.insert("is_test".into(), PropValue::Bool(self.is_in_test_mod()));
        props.insert("visibility".into(), PropValue::Str("private".into()));
        props.insert("impl_target".into(), PropValue::Str(target.into()));
        props.insert(
            "target".into(),
            PropValue::Str(self.target.as_wire_str().into_owned()),
        );
        if let Some(t) = trait_qname {
            props.insert("impl_trait".into(), PropValue::Str(t.into()));
        }
        insert_attr_metadata_props(&mut props, attrs);

        self.emitter.emit_node(Node {
            id: impl_id.clone(),
            label: Label::new(Label::ITEM),
            props,
        });
        self.emitter.claim_item_qname(&impl_qname, &self.target);
        self.emitter.emit_edge(Edge {
            src: impl_id.clone(),
            dst: self.crate_id.clone(),
            label: EdgeLabel::new(EdgeLabel::IN_CRATE),
            props: BTreeMap::new(),
        });
        self.emit_in_module_edge(&impl_id);

        let target_qname = resolve_target_qname(&self.module_stack, target);
        self.emitter.emit_edge(Edge {
            src: impl_id.clone(),
            dst: item_node_id(&target_qname),
            label: EdgeLabel::new(EdgeLabel::IMPLEMENTS_FOR),
            props: BTreeMap::new(),
        });

        if let Some(t) = trait_qname {
            let trait_resolved = resolve_target_qname(&self.module_stack, t);
            let mut props = BTreeMap::new();
            props.insert("resolver".to_string(), PropValue::Str("syn".to_string()));
            self.emitter.emit_edge(Edge {
                src: impl_id,
                dst: item_node_id(&trait_resolved),
                label: EdgeLabel::new(EdgeLabel::IMPLEMENTS),
                props,
            });
        }
    }

    pub(super) fn emit_attr_call_site(
        &mut self,
        parent_qname: &str,
        field_name: &str,
        callee_path: &str,
        kind: &str,
        line: usize,
    ) {
        let cs_id = cfdb_core::qname::callsite_node_id(
            &format!("{}.{field_name}", self.target.identity(parent_qname)),
            callee_path,
            0,
        );
        let mut extra = BTreeMap::new();
        extra.insert("field".into(), PropValue::Str(field_name.to_string()));
        emit_call_site_node_and_edge(
            self.emitter,
            cs_id,
            parent_qname,
            &self.target,
            callee_path,
            kind,
            self.file_path.clone(),
            line,
            self.is_in_test_mod(),
            extra,
        );
    }
}

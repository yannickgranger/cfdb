//! `syn::Visit` implementation for [`ItemVisitor`]. Split out of
//! `item_visitor.rs` to keep each file under the 500-LOC budget (#128).

use std::collections::BTreeMap;

use cfdb_core::fact::{Edge, Node, PropValue};
use cfdb_core::qname::{item_node_id, method_qname, normalize_impl_target, qname_from_node_id};
use cfdb_core::schema::{EdgeLabel, Label};
use syn::visit::Visit;

use crate::attrs::{
    attrs_contain_cfg_test, extract_cfg_feature_gate, extract_deprecated_attr, extract_path_attr,
    extract_serde_default_attr,
};
use crate::call_visitor::walk_call_sites_with_test_flag;
use crate::file_walker::PendingExternalMod;
use crate::type_render::{render_fn_signature, render_path, render_type_string};

use super::{parse_syn_visibility, span_line, ItemVisitor};

/// Extract `(name, is_self, type_path, type_normalized)` for one
/// `syn::FnArg`. Wildcard patterns (`_`) and non-ident patterns
/// collapse to an empty `name`. Receiver shape (`&self`, `&mut self`,
/// `self`) is rendered as `&Self`, `&mut Self`, or `Self` so cross-
/// extractor consumers see a stable string. §6.4 semantic
/// normalization is deferred; today `type_path` and `type_normalized`
/// share the rendered source form (#209 / RFC-036 §3.1).
fn param_info(arg: &syn::FnArg) -> (String, bool, String, String) {
    match arg {
        syn::FnArg::Receiver(r) => {
            let mut ty = String::new();
            if r.reference.is_some() {
                ty.push('&');
                if r.mutability.is_some() {
                    ty.push_str("mut ");
                }
            } else if r.mutability.is_some() {
                ty.push_str("mut ");
            }
            ty.push_str("Self");
            ("self".to_string(), true, ty.clone(), ty)
        }
        syn::FnArg::Typed(pt) => {
            let name = match pt.pat.as_ref() {
                syn::Pat::Ident(pi) => pi.ident.to_string(),
                _ => String::new(),
            };
            let ty = render_type_string(&pt.ty);
            (name, false, ty.clone(), ty)
        }
    }
}

impl<'ast> Visit<'ast> for ItemVisitor<'_> {
    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        let name = node.sig.ident.to_string();
        let is_test = self.fn_is_test(&node.attrs);
        let signature = render_fn_signature(&node.sig);
        let id = self.emit_item_with_flags(
            &name,
            "fn",
            span_line(&node.sig.ident),
            is_test,
            &node.vis,
            &node.attrs,
            Some(&signature),
        );
        let caller_qname = qname_from_node_id(&id).to_string();
        // RETURNS post-walk queue (RFC-037 §3.2, #216). Defer
        // resolution to `extract_workspace`'s post-walk pass — the
        // return type may name an item declared later in this file or
        // in a file walked later in the same workspace.
        if let syn::ReturnType::Type(_, ty) = &node.sig.output {
            let return_type = render_type_string(ty);
            self.emitter
                .deferred_returns
                .push((caller_qname.clone(), return_type));
        }
        for (index, arg) in node.sig.inputs.iter().enumerate() {
            let (name, is_self, type_path, type_normalized) = param_info(arg);
            self.emit_param(
                &caller_qname,
                index,
                &name,
                is_self,
                &type_path,
                &type_normalized,
            );
        }
        walk_call_sites_with_test_flag(
            self.emitter,
            &caller_qname,
            &self.file_path,
            &node.block,
            is_test,
        );
    }

    fn visit_item_impl(&mut self, node: &'ast syn::ItemImpl) {
        // Capture the impl target so nested method visits can build
        // qnames. Normalise through `cfdb_core::qname::normalize_impl_target`
        // so the stripped-angle-brackets form (`Vec` not `Vec<Node>`)
        // matches what `cfdb-hir-extractor` emits when it runs on the
        // same impl. Without this, generic impl targets produce
        // divergent qnames across the two extractors and cross-extractor
        // `CALLS(Item→Item)` edges silently dangle (#94 ddd review).
        let target = normalize_impl_target(&render_type_string(&node.self_ty));

        // #42 — emit an `:Item { kind: "impl_block" }` node for the impl
        // itself plus `IMPLEMENTS` (trait impls only) + `IMPLEMENTS_FOR`
        // edges. The impl-block node is the shared source for both
        // edges so queries can express "the trait-target pair for this
        // impl block" by joining `IMPLEMENTS` and `IMPLEMENTS_FOR` on
        // the impl-block id. (Inherent impls emit `IMPLEMENTS_FOR`
        // only — no trait to point to.)
        let trait_qname: Option<String> =
            node.trait_.as_ref().map(|(_, path, _)| render_path(path));
        self.emit_impl_block(&target, trait_qname.as_deref(), &node.attrs);

        let prev = self.current_impl_target.replace(target);
        syn::visit::visit_item_impl(self, node);
        self.current_impl_target = prev;
    }

    fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
        let method = node.sig.ident.to_string();
        let target = self
            .current_impl_target
            .clone()
            .unwrap_or_else(|| "_".to_string());
        // Method qname includes the impl target: `module::Foo::bar`.
        // We bypass emit_item() because it composes qname from
        // current_module_qpath() + name, which would drop the impl target.
        let qname = method_qname(&self.module_stack, &target, &method);
        let id = item_node_id(&qname);
        let is_test = self.fn_is_test(&node.attrs);
        let mut props = BTreeMap::new();
        props.insert("qname".into(), PropValue::Str(qname.clone()));
        props.insert("name".into(), PropValue::Str(method.clone()));
        props.insert("kind".into(), PropValue::Str("method".to_string()));
        props.insert("crate".into(), PropValue::Str(self.crate_name.clone()));
        props.insert(
            "bounded_context".into(),
            PropValue::Str(self.bounded_context.clone()),
        );
        props.insert(
            "module_qpath".into(),
            PropValue::Str(self.current_module_qpath()),
        );
        props.insert("impl_target".into(), PropValue::Str(target.clone()));
        props.insert("file".into(), PropValue::Str(self.file_path.clone()));
        props.insert(
            "line".into(),
            PropValue::Int(span_line(&node.sig.ident) as i64),
        );
        props.insert("is_test".into(), PropValue::Bool(is_test));
        props.insert(
            "visibility".into(),
            PropValue::Str(parse_syn_visibility(&node.vis).to_string()),
        );
        if let Some(gate) = extract_cfg_feature_gate(&node.attrs) {
            props.insert("cfg_gate".into(), PropValue::Str(gate.to_string()));
        }
        // Deprecation facts (#106) — impl-method path mirrors `emit_item_with_flags`.
        let (is_deprecated, deprecation_since) = extract_deprecated_attr(&node.attrs);
        props.insert("is_deprecated".into(), PropValue::Bool(is_deprecated));
        if let Some(since) = deprecation_since {
            props.insert("deprecation_since".into(), PropValue::Str(since));
        }
        // `:Item.signature` (#47) — impl-method path mirrors `emit_item_with_flags`.
        // Every `method`-kind Item carries a canonical signature string so
        // the `signature_divergent(a, b)` UDF can compare two items with
        // same last-segment qname across bounded contexts.
        props.insert(
            "signature".into(),
            PropValue::Str(render_fn_signature(&node.sig)),
        );
        self.emitter.emit_node(Node {
            id: id.clone(),
            label: Label::new(Label::ITEM),
            props,
        });
        // Track method qname for RETURNS / TYPE_OF post-walk
        // resolution (RFC-037 §3.2, #216). The impl-method emission
        // path bypasses `emit_item_with_flags`, so we insert into
        // the workspace-scoped set explicitly here.
        self.emitter.emitted_item_qnames.insert(qname.clone());
        self.emitter.emit_edge(Edge {
            src: id,
            dst: self.crate_id.clone(),
            label: EdgeLabel::new(EdgeLabel::IN_CRATE),
            props: BTreeMap::new(),
        });
        // RETURNS post-walk queue (RFC-037 §3.2, #216). Mirrors the
        // free-fn path in `visit_item_fn`. The deferred entry uses the
        // method's full qname (`module::Foo::bar`) so the post-walk
        // pass produces the correct `item:<method-qname>` src id.
        if let syn::ReturnType::Type(_, ty) = &node.sig.output {
            let return_type = render_type_string(ty);
            self.emitter
                .deferred_returns
                .push((qname.clone(), return_type));
        }
        for (index, arg) in node.sig.inputs.iter().enumerate() {
            let (name, is_self, type_path, type_normalized) = param_info(arg);
            self.emit_param(&qname, index, &name, is_self, &type_path, &type_normalized);
        }
        walk_call_sites_with_test_flag(self.emitter, &qname, &self.file_path, &node.block, is_test);
    }

    fn visit_item_struct(&mut self, node: &'ast syn::ItemStruct) {
        let name = node.ident.to_string();
        let id = self.emit_item(
            &name,
            "struct",
            span_line(&node.ident),
            &node.vis,
            &node.attrs,
        );
        let parent_qname = qname_from_node_id(&id).to_string();
        if let syn::Fields::Named(named) = &node.fields {
            for (index, f) in named.named.iter().enumerate() {
                if let Some(ident) = &f.ident {
                    let field_name = ident.to_string();
                    let ty = render_type_string(&f.ty);
                    // `type_normalized` and `type_path` receive the same
                    // string today (both produced by `render_type_string`);
                    // the split becomes meaningful when `render_type_inner`
                    // lands per RFC-037 §6 non-goals.
                    self.emit_field(&parent_qname, index, &field_name, &ty, &ty);
                    // Serde `default = "path"` attribute on a field is a
                    // name-based reference to a callable — syntactically
                    // visible to syn but never exercised as an `ExprCall`,
                    // so the CallSiteVisitor would miss it. Emit a
                    // `kind="serde_default"` CallSite linked from the
                    // owning struct Item so ban rules can catch it.
                    if let Some(callee_path) = extract_serde_default_attr(&f.attrs) {
                        self.emit_attr_call_site(
                            &parent_qname,
                            &field_name,
                            &callee_path,
                            "serde_default",
                        );
                    }
                }
            }
        }
    }

    fn visit_item_enum(&mut self, node: &'ast syn::ItemEnum) {
        let name = node.ident.to_string();
        self.emit_item(
            &name,
            "enum",
            span_line(&node.ident),
            &node.vis,
            &node.attrs,
        );
    }

    fn visit_item_trait(&mut self, node: &'ast syn::ItemTrait) {
        let name = node.ident.to_string();
        self.emit_item(
            &name,
            "trait",
            span_line(&node.ident),
            &node.vis,
            &node.attrs,
        );
    }

    fn visit_item_type(&mut self, node: &'ast syn::ItemType) {
        let name = node.ident.to_string();
        self.emit_item(
            &name,
            "type_alias",
            span_line(&node.ident),
            &node.vis,
            &node.attrs,
        );
    }

    fn visit_item_const(&mut self, node: &'ast syn::ItemConst) {
        let name = node.ident.to_string();
        self.emit_item(
            &name,
            "const",
            span_line(&node.ident),
            &node.vis,
            &node.attrs,
        );
    }

    fn visit_item_static(&mut self, node: &'ast syn::ItemStatic) {
        let name = node.ident.to_string();
        self.emit_item(
            &name,
            "static",
            span_line(&node.ident),
            &node.vis,
            &node.attrs,
        );
    }

    fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
        let mod_name = node.ident.to_string();
        let is_test_mod = attrs_contain_cfg_test(&node.attrs);
        self.module_stack.push(mod_name.clone());
        if is_test_mod {
            self.test_mod_depth += 1;
        }

        // Emit the module node + IN_MODULE membership for the parent module.
        let qpath = self.current_module_qpath();
        let id = format!("module:{qpath}");
        let mut props = BTreeMap::new();
        props.insert("qpath".into(), PropValue::Str(qpath));
        props.insert("name".into(), PropValue::Str(mod_name.clone()));
        props.insert("crate".into(), PropValue::Str(self.crate_name.clone()));
        props.insert("is_test".into(), PropValue::Bool(self.is_in_test_mod()));
        self.emitter.emit_node(Node {
            id: id.clone(),
            label: Label::new(Label::MODULE),
            props,
        });
        self.emitter.emit_edge(Edge {
            src: id,
            dst: self.crate_id.clone(),
            label: EdgeLabel::new(EdgeLabel::IN_CRATE),
            props: BTreeMap::new(),
        });

        if node.content.is_some() {
            syn::visit::visit_item_mod(self, node);
        } else {
            // External module (`mod foo;`) — not walked here; the caller
            // resolves and visits the file separately before/after this node.
            let path_override = extract_path_attr(&node.attrs);
            self.pending_external_mods.push(PendingExternalMod {
                name: mod_name,
                path_override,
                is_test: is_test_mod,
            });
        }

        self.module_stack.pop();
        if is_test_mod {
            self.test_mod_depth -= 1;
        }
    }
}

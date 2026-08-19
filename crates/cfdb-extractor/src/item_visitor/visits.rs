use std::collections::BTreeMap;

use cfdb_core::fact::{Edge, Node, PropValue};
use cfdb_core::qname::normalize_impl_target;
use cfdb_core::schema::{EdgeLabel, Label};
use syn::visit::Visit;

use crate::attrs::{attrs_contain_cfg_test, extract_path_attr, extract_serde_default_attr};
use crate::call_visitor::walk_call_sites_with_test_flag;
use crate::file_walker::PendingExternalMod;
use crate::literal_visitor::{walk_literals_in_block, walk_literals_in_expr};
use crate::match_visitor::walk_match_sites_with_test_flag;
use crate::type_render::{render_fn_signature, render_path, render_type_string};

use super::{span_line, ItemVisitor};

fn param_info(arg: &syn::FnArg) -> (String, bool, String, String, Option<syn::Type>) {
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
            ("self".to_string(), true, ty.clone(), ty, None)
        }
        syn::FnArg::Typed(pt) => {
            let name = match pt.pat.as_ref() {
                syn::Pat::Ident(pi) => pi.ident.to_string(),
                _ => String::new(),
            };
            let ty = render_type_string(&pt.ty);
            (name, false, ty.clone(), ty, Some((*pt.ty).clone()))
        }
    }
}

impl<'ast> Visit<'ast> for ItemVisitor<'_> {
    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        let name = node.sig.ident.to_string();
        let is_test = self.fn_is_test(&node.attrs);
        let signature = render_fn_signature(&node.sig);
        let (_id, caller_qname) = self.emit_item_with_flags(
            &name,
            "fn",
            span_line(&node.sig.ident),
            is_test,
            &node.vis,
            &node.attrs,
            Some(&signature),
            None,
        );
        if let syn::ReturnType::Type(_, ty) = &node.sig.output {
            let return_type = render_type_string(ty);
            self.emitter.deferred_returns.push((
                caller_qname.clone(),
                self.target.clone(),
                return_type,
                (**ty).clone(),
            ));
        }
        for (index, arg) in node.sig.inputs.iter().enumerate() {
            let (name, is_self, type_path, type_normalized, syn_type) = param_info(arg);
            self.emit_param(
                &caller_qname,
                index,
                &name,
                is_self,
                &type_path,
                &type_normalized,
                syn_type.as_ref(),
            );
        }
        walk_call_sites_with_test_flag(
            self.emitter,
            &caller_qname,
            &self.target,
            &self.file_path,
            &node.block,
            is_test,
        );
        walk_literals_in_block(
            self.emitter,
            &self.file_path,
            &self.crate_name,
            &node.block,
            is_test,
        );
        walk_match_sites_with_test_flag(
            self.emitter,
            &caller_qname,
            &self.target,
            &self.file_path,
            &self.crate_name,
            &node.block,
            is_test,
        );
    }

    fn visit_item_impl(&mut self, node: &'ast syn::ItemImpl) {
        let target = normalize_impl_target(&render_type_string(&node.self_ty));

        let trait_qname: Option<String> =
            node.trait_.as_ref().map(|(_, path, _)| render_path(path));
        let impl_line = node.impl_token.span.start().line;
        self.emit_impl_block(&target, trait_qname.as_deref(), impl_line, &node.attrs);

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
        let is_test = self.fn_is_test(&node.attrs);
        let signature = render_fn_signature(&node.sig);
        let (_id, qname) = self.emit_item_with_flags(
            &method,
            "method",
            span_line(&node.sig.ident),
            is_test,
            &node.vis,
            &node.attrs,
            Some(&signature),
            Some(&target),
        );
        if let syn::ReturnType::Type(_, ty) = &node.sig.output {
            let return_type = render_type_string(ty);
            self.emitter.deferred_returns.push((
                qname.clone(),
                self.target.clone(),
                return_type,
                (**ty).clone(),
            ));
        }
        for (index, arg) in node.sig.inputs.iter().enumerate() {
            let (name, is_self, type_path, type_normalized, syn_type) = param_info(arg);
            self.emit_param(
                &qname,
                index,
                &name,
                is_self,
                &type_path,
                &type_normalized,
                syn_type.as_ref(),
            );
        }
        walk_call_sites_with_test_flag(
            self.emitter,
            &qname,
            &self.target,
            &self.file_path,
            &node.block,
            is_test,
        );
        walk_literals_in_block(
            self.emitter,
            &self.file_path,
            &self.crate_name,
            &node.block,
            is_test,
        );
        walk_match_sites_with_test_flag(
            self.emitter,
            &qname,
            &self.target,
            &self.file_path,
            &self.crate_name,
            &node.block,
            is_test,
        );
    }

    fn visit_item_struct(&mut self, node: &'ast syn::ItemStruct) {
        let name = node.ident.to_string();
        let (id, parent_qname) = self.emit_item(
            &name,
            "struct",
            span_line(&node.ident),
            &node.vis,
            &node.attrs,
        );
        self.emit_field_list(&id, &node.fields, &parent_qname);
        if let syn::Fields::Named(named) = &node.fields {
            for f in &named.named {
                if let Some(ident) = &f.ident {
                    if let Some(callee_path) = extract_serde_default_attr(&f.attrs) {
                        let field_line = ident.span().start().line;
                        self.emit_attr_call_site(
                            &parent_qname,
                            &ident.to_string(),
                            &callee_path,
                            "serde_default",
                            field_line,
                        );
                    }
                }
            }
        }
    }

    fn visit_item_enum(&mut self, node: &'ast syn::ItemEnum) {
        let name = node.ident.to_string();
        let (_id, enum_qname) = self.emit_item(
            &name,
            "enum",
            span_line(&node.ident),
            &node.vis,
            &node.attrs,
        );
        for (index, variant) in node.variants.iter().enumerate() {
            let variant_name = variant.ident.to_string();
            let payload_kind = match &variant.fields {
                syn::Fields::Unit => "unit",
                syn::Fields::Unnamed(_) => "tuple",
                syn::Fields::Named(_) => "struct",
            };
            let (variant_id, variant_qname) =
                self.emit_variant(&enum_qname, index, &variant_name, payload_kind);
            self.emit_field_list(&variant_id, &variant.fields, &variant_qname);
        }
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
        let (item_id, _qname) = self.emit_item(
            &name,
            "const",
            span_line(&node.ident),
            &node.vis,
            &node.attrs,
        );
        if let Some(table) = crate::const_table::recognize_const_table(
            node,
            &self.crate_name,
            &self.current_module_qpath(),
            self.is_in_test_mod(),
        ) {
            self.emit_const_table(table, &item_id);
        }
        walk_literals_in_expr(
            self.emitter,
            &self.file_path,
            &self.crate_name,
            &node.expr,
            self.is_in_test_mod(),
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
        walk_literals_in_expr(
            self.emitter,
            &self.file_path,
            &self.crate_name,
            &node.expr,
            self.is_in_test_mod(),
        );
    }

    fn visit_item_union(&mut self, node: &'ast syn::ItemUnion) {
        let name = node.ident.to_string();
        self.emit_item(
            &name,
            "union",
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

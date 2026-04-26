//! `syn::Visit` implementation for [`ItemVisitor`]. Split out of
//! `item_visitor.rs` to keep each file under the 500-LOC budget (#128).

use std::collections::BTreeMap;

use cfdb_core::fact::{Edge, Node, PropValue};
use cfdb_core::qname::{normalize_impl_target, qname_from_node_id};
use cfdb_core::schema::{EdgeLabel, Label};
use syn::visit::Visit;

use crate::attrs::{attrs_contain_cfg_test, extract_path_attr, extract_serde_default_attr};
use crate::call_visitor::walk_call_sites_with_test_flag;
use crate::file_walker::PendingExternalMod;
use crate::type_render::{render_fn_signature, render_path, render_type_string};

use super::{span_line, ItemVisitor};

/// Extract `(name, is_self, type_path, type_normalized, syn_type)` for
/// one `syn::FnArg`. Wildcard patterns (`_`) and non-ident patterns
/// collapse to an empty `name`. Receiver shape (`&self`, `&mut self`,
/// `self`) is rendered as `&Self`, `&mut Self`, or `Self` so cross-
/// extractor consumers see a stable string; receivers have no
/// `syn::Type` (they carry `Self`), so `syn_type` is `None` in that
/// arm. §6.4 semantic normalization is deferred; today `type_path`
/// and `type_normalized` share the rendered source form (#209 /
/// RFC-036 §3.1). The `syn_type` slot is consumed downstream by
/// `emit_param` to power the TYPE_OF third-tier wrapper unwrap
/// (#239, RFC-037 §6 closeout).
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
        let id = self.emit_item_with_flags(
            &name,
            "fn",
            span_line(&node.sig.ident),
            is_test,
            &node.vis,
            &node.attrs,
            Some(&signature),
            None,
        );
        let caller_qname = qname_from_node_id(&id).to_string();
        // RETURNS post-walk queue (RFC-037 §3.2, #216). Defer
        // resolution to `extract_workspace`'s post-walk pass — the
        // return type may name an item declared later in this file or
        // in a file walked later in the same workspace.
        if let syn::ReturnType::Type(_, ty) = &node.sig.output {
            let return_type = render_type_string(ty);
            // Store the original `syn::Type` alongside the rendered
            // string so `resolve_deferred_returns` can fall back to
            // `render_type_inner` on wrapper unwrap (#239).
            self.emitter
                .deferred_returns
                .push((caller_qname.clone(), return_type, (**ty).clone()));
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
        // REGISTERS_PARAM for MCP `#[tool]` fns is emitted HIR-side in
        // `cfdb-hir-extractor::entry_point_emitter` — it owns
        // `:EntryPoint` node emission and therefore has a valid src id
        // in the keyspace. Emitting from here would produce dangling
        // edges that `cfdb-petgraph::ingest_one_edge` drops silently
        // (graph.rs:204), making the producer invisible in the graph.
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
        // Use the `impl` keyword's span line — stable across inherent
        // and trait impls, and matches what a human reader would point
        // to as "the impl line" (#273 / F-005).
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
        // Route through `emit_item_with_flags` with `impl_target = Some(&target)`
        // so the qname picks up the impl-target segment (`module::Foo::bar`)
        // and the `impl_target` prop lands on the `:Item` node. This is the
        // single owner of `:Item` prop emission — IN_CRATE, IN_MODULE,
        // emitted_item_qnames, deprecation, cfg_gate, signature, and
        // visibility are all owned by the helper (audit 2026-W17 / EPIC
        // #273 / Pattern 3 F-002 — eliminates ~95 lines of triplication).
        let id = self.emit_item_with_flags(
            &method,
            "method",
            span_line(&node.sig.ident),
            is_test,
            &node.vis,
            &node.attrs,
            Some(&signature),
            Some(&target),
        );
        let qname = qname_from_node_id(&id).to_string();
        // RETURNS post-walk queue (RFC-037 §3.2, #216). Mirrors the
        // free-fn path in `visit_item_fn`. The deferred entry uses the
        // method's full qname (`module::Foo::bar`) so the post-walk
        // pass produces the correct `item:<method-qname>` src id.
        if let syn::ReturnType::Type(_, ty) = &node.sig.output {
            let return_type = render_type_string(ty);
            // Store the original `syn::Type` alongside the rendered
            // string so `resolve_deferred_returns` can fall back to
            // `render_type_inner` on wrapper unwrap (#239).
            self.emitter
                .deferred_returns
                .push((qname.clone(), return_type, (**ty).clone()));
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
        // Walk the struct's fields uniformly — `emit_field_list` handles
        // both `Fields::Named` (record struct) and `Fields::Unnamed`
        // (tuple struct). `Fields::Unit` is a no-op. #218 / RFC-037 §3.3
        // step 7.
        self.emit_field_list(&id, &node.fields, &parent_qname);
        // Serde `default = "path"` attribute on a named field is a
        // name-based reference to a callable — syntactically visible
        // to syn but never exercised as an `ExprCall`, so the
        // CallSiteVisitor would miss it. Emit a `kind="serde_default"`
        // CallSite linked from the owning struct Item so ban rules can
        // catch it. Only applies to record-style fields (has `ident`).
        if let syn::Fields::Named(named) = &node.fields {
            for f in &named.named {
                if let Some(ident) = &f.ident {
                    if let Some(callee_path) = extract_serde_default_attr(&f.attrs) {
                        // Real source line of the field ident — the
                        // attr-ref CallSite points at the field whose
                        // `#[serde(default = "...")]` attr it represents
                        // (#273 / F-005).
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
        let id = self.emit_item(
            &name,
            "enum",
            span_line(&node.ident),
            &node.vis,
            &node.attrs,
        );
        let enum_qname = qname_from_node_id(&id).to_string();
        // Walk every variant — emit the `:Variant` node + `HAS_VARIANT`
        // edge, then recurse into the variant's payload via
        // `emit_field_list` (shared with `visit_item_struct`). #218 /
        // RFC-037 §3.3.
        for (index, variant) in node.variants.iter().enumerate() {
            let variant_name = variant.ident.to_string();
            let payload_kind = match &variant.fields {
                syn::Fields::Unit => "unit",
                syn::Fields::Unnamed(_) => "tuple",
                syn::Fields::Named(_) => "struct",
            };
            let (variant_id, variant_qname) =
                self.emit_variant(&enum_qname, index, &variant_name, payload_kind);
            // Variant payload fields use the `:Variant` node as the
            // `HAS_FIELD` edge src (the descriptor's widened `from:`
            // list). `parent_qname` is `Enum::Variant` so field ids
            // (`field:Enum::Variant.x`) do not collide with enum- or
            // struct-field ids on the same graph.
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
        let item_id = self.emit_item(
            &name,
            "const",
            span_line(&node.ident),
            &node.vis,
            &node.attrs,
        );
        // RFC-040 §3.3 — recognize literal slice/array tables and emit a
        // `:ConstTable` node + `HAS_CONST_TABLE` edge alongside the parent
        // `:Item`. Non-recognized consts (scalars, custom types, non-literal
        // exprs) take the early-return None path and emit only the parent.
        if let Some(table) = crate::const_table::recognize_const_table(
            node,
            &self.crate_name,
            &self.current_module_qpath(),
            self.is_in_test_mod(),
        ) {
            self.emit_const_table(table, &item_id);
        }
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

        // Emit the `:Module` node and its `IN_CRATE` edge to the owning
        // `:Crate`. `:Module` itself does NOT emit an `IN_MODULE` edge
        // to its parent module — `IN_MODULE` is declared from `[Item,
        // File]` to `[Module]` (`cfdb-core/src/schema/describe/edges.rs`
        // — `Module → Module` would be a separate parentage edge,
        // intentionally not in v0.1 vocabulary). Item membership in
        // this module is emitted by the per-item helpers via
        // `emit_in_module_edge`.
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

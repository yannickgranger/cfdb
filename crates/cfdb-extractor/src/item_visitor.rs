//! `syn::Visit` implementation for module-level items. Drives `Item` /
//! `Module` / `Field` / `CallSite` emission and queues external `mod foo;`
//! declarations for the outer [`crate::file_walker`] to resolve and recurse
//! into.

use std::collections::BTreeMap;
use std::str::FromStr;

use cfdb_core::fact::{Edge, Node, PropValue};
use cfdb_core::qname::{
    item_node_id, item_qname, method_qname, module_qpath, normalize_impl_target, qname_from_node_id,
};
use cfdb_core::schema::{EdgeLabel, Label};
use cfdb_core::Visibility;
use syn::visit::Visit;

use crate::attrs::{
    attrs_contain_cfg_test, attrs_contain_hash_test, extract_cfg_feature_gate,
    extract_deprecated_attr, extract_path_attr, extract_serde_default_attr,
};
use crate::call_visitor::walk_call_sites_with_test_flag;
use crate::file_walker::PendingExternalMod;
use crate::type_render::{render_path, render_type_string};
use crate::Emitter;

pub(crate) struct ItemVisitor<'e> {
    pub(crate) emitter: &'e mut Emitter,
    pub(crate) crate_id: String,
    pub(crate) crate_name: String,
    pub(crate) file_path: String,
    /// Bounded context the containing crate belongs to — computed once per
    /// crate in [`crate::extract_workspace`] via
    /// [`cfdb_concepts::compute_bounded_context`] and propagated down through
    /// [`crate::file_walker::visit_file`]. Stamped onto every Item node at
    /// emission time (council-cfdb-wiring §B.1.2).
    pub(crate) bounded_context: String,
    /// Path of module names from crate root to current position. The first
    /// element is the crate name (dashes replaced with underscores), matching
    /// Rust's qname convention.
    pub(crate) module_stack: Vec<String>,
    /// External (`mod foo;`) declarations encountered while visiting this
    /// file. Each carries its name, optional `#[path]` override, and
    /// whether it was under `#[cfg(test)]`. The caller resolves each to
    /// a child file and recurses, inheriting the test flag so every
    /// Item/CallSite beneath it is tagged correctly.
    pub(crate) pending_external_mods: Vec<PendingExternalMod>,
    /// Set while inside an `impl` block — the textual rendering of the impl
    /// target type. Used to build qnames for methods so `impl Foo { fn bar }`
    /// produces `module::Foo::bar` rather than `module::bar`.
    pub(crate) current_impl_target: Option<String>,
    /// Depth counter for nested `#[cfg(test)]` (or `#[cfg(all(test, ...))]`)
    /// module scopes. `> 0` means every Item/CallSite emitted right now is
    /// test code. This is the signal that lets `arch-ban-*` rules filter
    /// out test modules without resorting to qname regex hacks.
    pub(crate) test_mod_depth: u32,
}

impl ItemVisitor<'_> {
    fn current_module_qpath(&self) -> String {
        module_qpath(&self.module_stack)
    }

    fn qname(&self, item_name: &str) -> String {
        item_qname(&self.module_stack, item_name)
    }

    fn is_in_test_mod(&self) -> bool {
        self.test_mod_depth > 0
    }

    /// `is_test` for a fn item: true when either (a) the enclosing module is
    /// `#[cfg(test)]`-gated, or (b) the fn itself carries a bare `#[test]`
    /// attribute. This is the single OR site — non-fn items stay on the
    /// module-depth signal alone (struct/enum/etc. have no libtest-native
    /// marker). Council-cfdb-wiring §B.1.1.
    fn fn_is_test(&self, attrs: &[syn::Attribute]) -> bool {
        self.is_in_test_mod() || attrs_contain_hash_test(attrs)
    }

    fn emit_item(
        &mut self,
        name: &str,
        kind: &str,
        line: usize,
        vis: &syn::Visibility,
        attrs: &[syn::Attribute],
    ) -> String {
        self.emit_item_with_flags(name, kind, line, self.is_in_test_mod(), vis, attrs)
    }

    /// Like [`emit_item`] but the caller supplies the `is_test` flag
    /// explicitly. Used by the fn-item visit path so a bare `#[test]` fn
    /// outside a `#[cfg(test)]` module is still tagged `is_test=true`.
    fn emit_item_with_flags(
        &mut self,
        name: &str,
        kind: &str,
        line: usize,
        is_test: bool,
        vis: &syn::Visibility,
        attrs: &[syn::Attribute],
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
    fn emit_impl_block(
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
    fn emit_attr_call_site(
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

    fn emit_field(&mut self, parent_qname: &str, name: &str, ty: &str) {
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

impl<'ast> Visit<'ast> for ItemVisitor<'_> {
    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        let name = node.sig.ident.to_string();
        let is_test = self.fn_is_test(&node.attrs);
        let id = self.emit_item_with_flags(
            &name,
            "fn",
            span_line(&node.sig.ident),
            is_test,
            &node.vis,
            &node.attrs,
        );
        let caller_qname = qname_from_node_id(&id).to_string();
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
        self.emitter.emit_node(Node {
            id: id.clone(),
            label: Label::new(Label::ITEM),
            props,
        });
        self.emitter.emit_edge(Edge {
            src: id,
            dst: self.crate_id.clone(),
            label: EdgeLabel::new(EdgeLabel::IN_CRATE),
            props: BTreeMap::new(),
        });
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
            for f in &named.named {
                if let Some(ident) = &f.ident {
                    let field_name = ident.to_string();
                    let ty = render_type_string(&f.ty);
                    self.emit_field(&parent_qname, &field_name, &ty);
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

/// Build the `qname` for an `impl` block (#42). The segments combine the
/// current module path, the normalised target type, and the canonical
/// `impl[_<Trait>]` suffix — yielding a stable, human-readable id that
/// disambiguates inherent impls from each distinct trait impl on the
/// same target:
///
/// - `impl Foo { ... }` at module `m`        → `m::Foo::impl`
/// - `impl Display for Foo { ... }`          → `m::Foo::impl_Display`
/// - `impl crate::bar::Trait for Foo { ... }` → `m::Foo::impl_crate_bar_Trait`
fn impl_block_qname(module_stack: &[String], target: &str, trait_qname: Option<&str>) -> String {
    let module = module_qpath(module_stack);
    let prefix = if module.is_empty() {
        String::new()
    } else {
        format!("{module}::")
    };
    let trait_segment = trait_qname
        .map(|t| format!("_{}", t.replace("::", "_")))
        .unwrap_or_default();
    format!("{prefix}{target}::impl{trait_segment}")
}

/// Human-readable `name` prop for an impl-block :Item node (#42). Mirrors
/// Rust source-level rendering: `impl Foo` (inherent) or
/// `impl Bar for Foo` (trait impl).
fn impl_block_name(target: &str, trait_qname: Option<&str>) -> String {
    match trait_qname {
        Some(t) => format!("impl {t} for {target}"),
        None => format!("impl {target}"),
    }
}

/// Resolve a bare type/trait name (as written in source) into the full
/// qname formula used by [`item_qname`]. For an unqualified segment like
/// `"Polite"`, the current crate + module prefix is prepended so the
/// resulting `item:<qname>` id matches what the struct/trait emitters
/// produce. Already-qualified inputs (containing `::`) pass through
/// unchanged — they may dangle when they point outside the workspace,
/// which the petgraph ingest layer handles with a non-fatal warning.
fn resolve_target_qname(module_stack: &[String], type_or_trait: &str) -> String {
    if type_or_trait.contains("::") {
        return type_or_trait.to_string();
    }
    item_qname(module_stack, type_or_trait)
}

fn span_line(_ident: &syn::Ident) -> usize {
    // proc_macro2::Span does not expose line info on stable Rust. Storing 0
    // is a known placeholder that callers can overwrite later with a
    // rustc-generated source map. RFC §8.2 phase B tracks this.
    0
}

/// Translate a `syn::Visibility` AST node into the typed cfdb-core enum
/// (RFC-033 §7 A1 / Issue #35).
///
/// Two-step pipeline: render the AST to the canonical wire string, then
/// delegate to `Visibility::from_str`. This keeps a **single resolution
/// point** for the Rust→`Visibility` mapping — when the variant list grows,
/// only `Visibility`'s wire-str / `FromStr` pair needs updating, and the
/// extractor automatically picks up the new variant. Split-brain audit
/// (`audit-split-brain` FromStrBypass check) enforces the invariant.
///
/// Mapping (see `render_syn_visibility_wire` for the AST side and
/// `impl FromStr for Visibility` for the string side):
///
/// - `pub`                        → `Public`
/// - `pub(crate)`                 → `CrateLocal`
/// - `pub(super)` / `pub(self)`   → `Module` (semantic equivalence; wire
///   always renders as `pub(super)`)
/// - inherited (no modifier)      → `Private`
/// - `pub(in path::to::mod)` and any other `Restricted` path → `Restricted`
///   carrying the `::`-joined path string
fn parse_syn_visibility(vis: &syn::Visibility) -> Visibility {
    let wire = render_syn_visibility_wire(vis);
    Visibility::from_str(&wire).expect(
        "render_syn_visibility_wire produces canonical wire strings that FromStr accepts — \
         if this panics, the two sides of the visibility mapping drifted and audit-split-brain \
         should have caught it",
    )
}

/// Render a `syn::Visibility` AST node to its canonical wire string
/// (see `Visibility::as_wire_str` for the inverse + full grammar). Kept
/// separate from `parse_syn_visibility` so tests can assert the rendering
/// alone without the FromStr round-trip.
fn render_syn_visibility_wire(vis: &syn::Visibility) -> String {
    match vis {
        syn::Visibility::Public(_) => "pub".to_string(),
        syn::Visibility::Inherited => "private".to_string(),
        syn::Visibility::Restricted(r) => {
            let segments: Vec<String> = r
                .path
                .segments
                .iter()
                .map(|s| s.ident.to_string())
                .collect();
            // `pub(in crate)` / `pub(in super)` / `pub(in self)` — the
            // `in` keyword makes these canonically-path-restricted. syn
            // distinguishes them from the shorter `pub(crate)` /
            // `pub(super)` / `pub(self)` forms via `r.in_token.is_some()`.
            // The short form matches on a single-segment path without the
            // `in` keyword; the long form always keeps the path verbatim.
            let has_in = r.in_token.is_some();
            match (segments.len(), segments.first().map(String::as_str), has_in) {
                (1, Some("crate"), false) => "pub(crate)".to_string(),
                (1, Some("super"), false) | (1, Some("self"), false) => "pub(super)".to_string(),
                _ => format!("pub(in {})", segments.join("::")),
            }
        }
    }
}

#[cfg(test)]
mod parse_syn_visibility_tests {
    use super::parse_syn_visibility;
    use cfdb_core::Visibility;

    fn parse(src: &str) -> syn::Visibility {
        // Parse via a wrapper item so the visibility appears in a
        // well-formed context syn accepts.
        let wrapped = format!("{src} fn dummy() {{}}");
        let item: syn::ItemFn = syn::parse_str(&wrapped).expect("parse test fixture");
        item.vis
    }

    #[test]
    fn inherited_is_private() {
        assert_eq!(parse_syn_visibility(&parse("")), Visibility::Private);
    }

    #[test]
    fn pub_is_public() {
        assert_eq!(parse_syn_visibility(&parse("pub")), Visibility::Public);
    }

    #[test]
    fn pub_crate_is_crate_local() {
        assert_eq!(
            parse_syn_visibility(&parse("pub(crate)")),
            Visibility::CrateLocal
        );
    }

    #[test]
    fn pub_super_and_pub_self_collapse_to_module() {
        assert_eq!(
            parse_syn_visibility(&parse("pub(super)")),
            Visibility::Module
        );
        assert_eq!(
            parse_syn_visibility(&parse("pub(self)")),
            Visibility::Module
        );
    }

    #[test]
    fn pub_in_path_is_restricted() {
        assert_eq!(
            parse_syn_visibility(&parse("pub(in crate::foo::bar)")),
            Visibility::Restricted("crate::foo::bar".into())
        );
    }

    #[test]
    fn pub_in_crate_does_not_collapse_to_crate_local() {
        // `pub(in crate)` is a restricted-path form, not the short
        // `pub(crate)`. We preserve the distinction on the wire.
        assert_eq!(
            parse_syn_visibility(&parse("pub(in crate)")),
            Visibility::Restricted("crate".into())
        );
    }
}

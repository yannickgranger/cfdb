//! CallSite extraction inside a function body.
//!
//! `walk_call_sites_with_test_flag` is the entry point — it wraps the
//! emitter in a [`CallSiteVisitor`] and drives `syn::visit::visit_block`.
//! The visitor emits one `:CallSite` node per call expression, per
//! fn-pointer argument, and per expression-carrying macro body it can
//! re-parse. See `item_visitor.rs` for the invokers.

use std::collections::BTreeMap;

use cfdb_core::fact::{Edge, Node, PropValue};
use cfdb_core::qname::item_node_id;
use cfdb_core::schema::{EdgeLabel, Label};
use syn::visit::Visit;

use crate::type_render::render_path;
use crate::Emitter;

/// Walk a function block, emit a `:CallSite` node per call expression, and
/// link it from the caller Item via `INVOKES_AT`.
///
/// **Name-based, unresolved.** We rely on `syn`'s textual view — the callee
/// path is whatever the author wrote at the call site (`Utc::now`,
/// `chrono::Utc::now`, `self.clock.now`). For method calls (`x.foo()`) we
/// record the method name only; the receiver is an expression, not a path,
/// and rendering it reliably requires type resolution (Phase B).
///
/// `is_test` propagates the enclosing `#[cfg(test)]` depth flag to every
/// emitted CallSite so rules can filter prod-vs-test cleanly.
pub(crate) fn walk_call_sites_with_test_flag(
    emitter: &mut Emitter,
    caller_qname: &str,
    file_path: &str,
    block: &syn::Block,
    is_test: bool,
) {
    let mut visitor = CallSiteVisitor {
        emitter,
        caller_qname,
        file_path,
        counts: BTreeMap::new(),
        is_test,
    };
    syn::visit::visit_block(&mut visitor, block);
}

struct CallSiteVisitor<'e, 'a> {
    emitter: &'e mut Emitter,
    caller_qname: &'a str,
    file_path: &'a str,
    /// Count of prior occurrences of each `callee_path` within this fn body.
    /// Used to build collision-free CallSite ids without needing source
    /// offsets (which `proc_macro2::Span` doesn't expose on stable Rust).
    counts: BTreeMap<String, usize>,
    is_test: bool,
}

impl<'ast> Visit<'ast> for CallSiteVisitor<'_, '_> {
    fn visit_expr_call(&mut self, node: &'ast syn::ExprCall) {
        if let syn::Expr::Path(p) = &*node.func {
            let callee_path = render_path(&p.path);
            self.emit_call_site(&callee_path, "call");
        }
        // Fn-pointer arg pattern: `foo(bar)` where `bar` is an `ExprPath`
        // that names a callable. syn's default visitor descends but never
        // emits a CallSite for the bare path, so `.unwrap_or_else(Utc::now)`
        // was a silent blind spot for every ban rule that filtered by
        // `callee_path`. Generous emission with `kind="fn_ptr"` fixes this;
        // consumers can filter by kind if they want only direct calls.
        for arg in &node.args {
            if let syn::Expr::Path(p) = arg {
                let path = render_path(&p.path);
                self.emit_call_site(&path, "fn_ptr");
            }
        }
        // Recurse into args — nested calls (`foo(bar())`) must not be lost.
        syn::visit::visit_expr_call(self, node);
    }

    fn visit_expr_method_call(&mut self, node: &'ast syn::ExprMethodCall) {
        let method = node.method.to_string();
        self.emit_call_site(&method, "method");
        // Same fn-pointer-arg projection as `visit_expr_call`. This is the
        // dominant shape in real code: `.unwrap_or_else(Utc::now)`,
        // `.or_insert_with(Default::default)`, etc.
        for arg in &node.args {
            if let syn::Expr::Path(p) = arg {
                let path = render_path(&p.path);
                self.emit_call_site(&path, "fn_ptr");
            }
        }
        syn::visit::visit_expr_method_call(self, node);
    }

    /// Re-parse macro invocation tokens so calls inside `vec![...]`,
    /// `json!(...)`, etc. are not invisible. Delegates to [`walk_macro_tokens`].
    fn visit_expr_macro(&mut self, node: &'ast syn::ExprMacro) {
        self.walk_macro_tokens(&node.mac);
    }

    /// Statement-position macro invocations — `assert_eq!(a, b);`,
    /// `println!(...);`, `tracing::info!(...);` — parse as `Stmt::Macro`,
    /// NOT `Expr::Macro`. syn's default `visit_stmt_macro` doesn't walk
    /// into tokens either, so without this override every call site
    /// inside a statement-level macro is invisible. Same delegation.
    fn visit_stmt_macro(&mut self, node: &'ast syn::StmtMacro) {
        self.walk_macro_tokens(&node.mac);
    }
}

impl CallSiteVisitor<'_, '_> {
    /// Re-parse a macro's token stream and recurse through any
    /// expressions found so call sites inside macro bodies become
    /// visible. Strategy: try three progressively-general parse shapes
    /// and walk whichever succeeds.
    ///
    /// Unparseable bodies (format strings without trailing exprs, DSL
    /// macros like `quote!`, declarative macro_rules bodies) fall
    /// through silently — this is best-effort by design. The goal is
    /// to catch the common expression-carrying macros (`vec!`, `json!`,
    /// `assert_eq!`, `format!` args, `tracing::info!` args), not to
    /// expand every macro.
    fn walk_macro_tokens(&mut self, mac: &syn::Macro) {
        use syn::parse::Parser;
        use syn::visit::Visit;
        let tokens = mac.tokens.clone();

        // (1) Punctuated<Expr, Comma> — catches vec!, json!, assert_eq!,
        //     and most function-like expression macros.
        let punct_parser =
            syn::punctuated::Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated;
        if let Ok(exprs) = punct_parser.parse2(tokens.clone()) {
            for expr in &exprs {
                // `expr` is owned by this parse, not the outer AST —
                // but Visit is parametric on 'ast so the lifetime of
                // the walked tree matches the local borrow.
                self.visit_expr(expr);
            }
            return;
        }

        // (2) Block shape — macros whose body is a block of statements
        //     like `{ let x = ...; foo(x) }`.
        if let Ok(block) = syn::parse2::<syn::Block>(tokens.clone()) {
            self.visit_block(&block);
            return;
        }

        // (3) Single expression fallback.
        if let Ok(expr) = syn::parse2::<syn::Expr>(tokens) {
            self.visit_expr(&expr);
        }
    }
}

impl CallSiteVisitor<'_, '_> {
    fn emit_call_site(&mut self, callee_path: &str, kind: &str) {
        let local_idx = {
            let counter = self.counts.entry(callee_path.to_string()).or_insert(0);
            let idx = *counter;
            *counter += 1;
            idx
        };
        let cs_id = format!(
            "callsite:{}:{}:{}",
            self.caller_qname, callee_path, local_idx
        );
        let last_segment = callee_path
            .rsplit("::")
            .next()
            .unwrap_or(callee_path)
            .to_string();

        let mut props = BTreeMap::new();
        props.insert(
            "caller_qname".into(),
            PropValue::Str(self.caller_qname.to_string()),
        );
        props.insert(
            "callee_path".into(),
            PropValue::Str(callee_path.to_string()),
        );
        props.insert("callee_last_segment".into(), PropValue::Str(last_segment));
        props.insert("kind".into(), PropValue::Str(kind.to_string()));
        props.insert("file".into(), PropValue::Str(self.file_path.to_string()));
        props.insert("line".into(), PropValue::Int(0));
        props.insert("is_test".into(), PropValue::Bool(self.is_test));
        // SchemaVersion v0.1.3+ discriminator (Label::CALL_SITE doc, #83).
        props.insert("resolver".into(), PropValue::Str("syn".to_string()));
        props.insert("callee_resolved".into(), PropValue::Bool(false));

        self.emitter.emit_node(Node {
            id: cs_id.clone(),
            label: Label::new(Label::CALL_SITE),
            props,
        });
        self.emitter.emit_edge(Edge {
            src: item_node_id(self.caller_qname),
            dst: cs_id,
            label: EdgeLabel::new(EdgeLabel::INVOKES_AT),
            props: BTreeMap::new(),
        });
    }
}

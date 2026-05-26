//! CallSite extraction inside a function body.
//!
//! `walk_call_sites_with_test_flag` is the entry point — it wraps the
//! emitter in a [`CallSiteVisitor`] and drives `syn::visit::visit_block`.
//! The visitor emits one `:CallSite` node per call expression, per
//! fn-pointer argument, and per expression-carrying macro body it can
//! re-parse. See `item_visitor.rs` for the invokers.

use std::collections::BTreeMap;

use cfdb_core::fact::{Edge, Node, PropValue};
use cfdb_core::qname::argument_node_id;
use cfdb_core::schema::{EdgeLabel, Label, RECEIVER_POSITION};
use cfdb_extractor_shared::classify_arg_kind;
use quote::ToTokens as _;
use syn::spanned::Spanned;
use syn::visit::Visit;

use crate::item_visitor::emit_call_site_node_and_edge;
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
    /// Used to build collision-free CallSite ids — even with real line
    /// numbers (#273 / F-005), two calls on the same line need distinct
    /// ids so the local-index counter stays.
    counts: BTreeMap<String, usize>,
    is_test: bool,
}

impl<'ast> Visit<'ast> for CallSiteVisitor<'_, '_> {
    fn visit_expr_call(&mut self, node: &'ast syn::ExprCall) {
        if let syn::Expr::Path(p) = &*node.func {
            let callee_path = render_path(&p.path);
            // Source-line of the call — `node.func.span()` points at the
            // callee path which is the "where the call happens" the
            // human reader expects (#273 / F-005).
            let line = node.func.span().start().line;
            let cs_id = self.emit_call_site(&callee_path, "call", line);
            // RFC-043 Slice A: emit :Argument nodes for each positional arg.
            // For ExprCall, position 0 is the first positional argument.
            self.emit_arguments(&cs_id, &node.args, 0);
        }
        // Fn-pointer arg pattern: `foo(bar)` where `bar` is an `ExprPath`
        // that names a callable. syn's default visitor descends but never
        // emits a CallSite for the bare path, so `.unwrap_or_else(Utc::now)`
        // was a silent blind spot for every ban rule that filtered by
        // `callee_path`. Generous emission with `kind="fn_ptr"` fixes this;
        // consumers can filter by kind if they want only direct calls.
        // No :Argument emission for fn_ptr sites — they represent a fn pointer
        // passed as an argument, not an invocation with an argument list.
        for arg in &node.args {
            if let syn::Expr::Path(p) = arg {
                let path = render_path(&p.path);
                let line = p.span().start().line;
                self.emit_call_site(&path, "fn_ptr", line);
            }
        }
        // Recurse into args — nested calls (`foo(bar())`) must not be lost.
        syn::visit::visit_expr_call(self, node);
    }

    fn visit_expr_method_call(&mut self, node: &'ast syn::ExprMethodCall) {
        let method = node.method.to_string();
        // The method ident span is the "where the call happens" line —
        // `x\n .foo()` has the receiver on one line and `.foo` on the
        // next; the method-name line is the more useful one for
        // line-precision queries (#273 / F-005).
        let line = node.method.span().start().line;
        let cs_id = self.emit_call_site(&method, "method", line);
        // RFC-043 Slice A: position 0 is the implicit self receiver
        // (RECEIVER_POSITION = 0); positions 1..N are the explicit args.
        self.emit_single_argument(&cs_id, &node.receiver, RECEIVER_POSITION);
        self.emit_arguments(&cs_id, &node.args, 1);
        // Same fn-pointer-arg projection as `visit_expr_call`. This is the
        // dominant shape in real code: `.unwrap_or_else(Utc::now)`,
        // `.or_insert_with(Default::default)`, etc.
        // No :Argument emission for fn_ptr sites — same rationale as above.
        for arg in &node.args {
            if let syn::Expr::Path(p) = arg {
                let path = render_path(&p.path);
                let arg_line = p.span().start().line;
                self.emit_call_site(&path, "fn_ptr", arg_line);
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
    fn emit_call_site(&mut self, callee_path: &str, kind: &str, line: usize) -> String {
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
        emit_call_site_node_and_edge(
            self.emitter,
            cs_id.clone(),
            self.caller_qname,
            callee_path,
            kind,
            self.file_path.to_string(),
            line,
            self.is_test,
            BTreeMap::new(),
        );
        cs_id
    }

    /// Emit `:Argument` nodes + `HAS_ARG` edges for a slice of expressions,
    /// starting at `position_offset`. Used for `ExprCall` args (offset 0)
    /// and for the explicit args of `ExprMethodCall` (offset 1, after the
    /// receiver which is emitted separately via `emit_single_argument`).
    fn emit_arguments(
        &mut self,
        cs_id: &str,
        args: &syn::punctuated::Punctuated<syn::Expr, syn::Token![,]>,
        position_offset: u32,
    ) {
        for (i, arg) in args.iter().enumerate() {
            self.emit_single_argument(cs_id, arg, position_offset + i as u32);
        }
    }

    /// Emit one `:Argument` node + `HAS_ARG` edge for a single expression.
    fn emit_single_argument(&mut self, cs_id: &str, expr: &syn::Expr, position: u32) {
        let arg_id = argument_node_id(cs_id, position);
        let span = expr.span();
        let loc = span.start();
        let source_text = expr.to_token_stream().to_string();
        let kind = classify_arg_kind(expr);

        let mut props = BTreeMap::new();
        props.insert("position".into(), PropValue::Int(i64::from(position)));
        props.insert("kind".into(), PropValue::Str(kind.to_string()));
        props.insert("source_text".into(), PropValue::Str(source_text));
        props.insert("file".into(), PropValue::Str(self.file_path.to_string()));
        props.insert("line".into(), PropValue::Int(loc.line as i64));
        // proc_macro2::LineColumn::column is 0-indexed; schema stores 1-indexed.
        props.insert("col".into(), PropValue::Int(loc.column as i64 + 1));

        self.emitter.emit_node(Node {
            id: arg_id.clone(),
            label: Label::new(Label::ARGUMENT),
            props,
        });
        self.emitter.emit_edge(Edge {
            src: cs_id.to_string(),
            dst: arg_id,
            label: EdgeLabel::new(EdgeLabel::HAS_ARG),
            props: BTreeMap::new(),
        });
    }
}

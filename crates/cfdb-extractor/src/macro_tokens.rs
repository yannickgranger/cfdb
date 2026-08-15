//! Shared macro-body re-parse helper.
//!
//! [`walk_macro_tokens`] re-parses a macro invocation's token stream and
//! drives the supplied `syn::Visit` visitor through any expressions it
//! finds, so facts inside `vec![...]`, `json!(...)`, `assert_eq!(...)`,
//! `tracing::info!(...)`, and any other expression-carrying macro
//! *invocation* become visible.
//!
//! Unparseable bodies (format strings without trailing exprs, DSL macros
//! like `quote!`, `macro_rules!` *definitions*) fall through silently:
//! this is best-effort by design. `macro_rules!` definition bodies are a
//! hard boundary — `syn::visit::Visit` never recurses into an `ItemMacro`
//! token tree. The goal is to catch the common expression-carrying macros,
//! not to expand every macro.

use syn::visit::Visit;

/// Re-parse `mac`'s token stream and walk `visitor` through it. Tries
/// three progressively-general parse shapes and drives whichever succeeds:
///
/// 1. `Punctuated<Expr, Comma>` — `vec!`, `json!`, `assert_eq!`, and most
///    function-like expression macros (also catches a lone expression
///    body, which parses as a one-element list).
/// 2. `Block` — macros whose body is a brace-delimited statement block.
/// 3. Single `Expr` — the fallback for a bare expression body.
///
/// The visitor bound is higher-ranked over the AST lifetime
/// (`for<'ast> Visit<'ast>`) because the re-parsed nodes are owned by this
/// call, not by the outer AST — the walked tree's lifetime is the local
/// borrow, exactly as the original per-visitor methods relied on.
pub(crate) fn walk_macro_tokens<V>(visitor: &mut V, mac: &syn::Macro)
where
    V: for<'ast> Visit<'ast>,
{
    use syn::parse::Parser;
    let tokens = mac.tokens.clone();

    // (1) Punctuated<Expr, Comma>.
    let punct_parser = syn::punctuated::Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated;
    if let Ok(exprs) = punct_parser.parse2(tokens.clone()) {
        for expr in &exprs {
            visitor.visit_expr(expr);
        }
        return;
    }
    // (2) Block shape — `{ let x = ...; foo(x) }`.
    if let Ok(block) = syn::parse2::<syn::Block>(tokens.clone()) {
        visitor.visit_block(&block);
        return;
    }
    // (3) Single-expression fallback.
    if let Ok(expr) = syn::parse2::<syn::Expr>(tokens) {
        visitor.visit_expr(&expr);
    }
}

#[cfg(test)]
mod tests {
    //! Unit tests for macro-token re-parse logic. A minimal recording
    //! visitor proves the helper re-parses a macro body and drives the
    //! visitor through the expressions it carries.

    use super::*;
    use syn::visit::Visit;

    /// Records the last-segment name of every call / method-call
    /// expression visited — enough to observe that the helper reached
    /// into the macro body.
    #[derive(Default)]
    struct CallRecorder {
        calls: Vec<String>,
    }

    impl<'ast> Visit<'ast> for CallRecorder {
        fn visit_expr_call(&mut self, node: &'ast syn::ExprCall) {
            if let syn::Expr::Path(p) = &*node.func {
                if let Some(seg) = p.path.segments.last() {
                    self.calls.push(seg.ident.to_string());
                }
            }
            syn::visit::visit_expr_call(self, node);
        }

        fn visit_expr_method_call(&mut self, node: &'ast syn::ExprMethodCall) {
            self.calls.push(node.method.to_string());
            syn::visit::visit_expr_method_call(self, node);
        }
    }

    /// Parse `name!( <tokens> )` and return the inner `syn::Macro`.
    fn macro_from(src: &str) -> syn::Macro {
        match syn::parse_str::<syn::Expr>(src).expect("valid macro invocation expression") {
            syn::Expr::Macro(m) => m.mac,
            other => panic!("expected Expr::Macro, got {other:?}"),
        }
    }

    #[test]
    fn punctuated_expr_body_is_walked() {
        // `vec![a(), b.c()]` → the helper re-parses `a(), b.c()` as a
        // comma-separated expr list and visits both.
        let mac = macro_from("vec![a(), b.c()]");
        let mut rec = CallRecorder::default();
        walk_macro_tokens(&mut rec, &mac);
        assert!(rec.calls.contains(&"a".to_string()));
        assert!(rec.calls.contains(&"c".to_string()));
    }

    #[test]
    fn single_expression_body_is_walked() {
        // A lone expression body still reaches the visitor (via the
        // one-element punctuated tier).
        let mac = macro_from("dbg!(foo())");
        let mut rec = CallRecorder::default();
        walk_macro_tokens(&mut rec, &mac);
        assert!(rec.calls.contains(&"foo".to_string()));
    }

    #[test]
    fn unparseable_body_is_silent() {
        // A DSL body that parses as none of the three shapes (an item,
        // not an expr/block) falls through without panicking and visits
        // nothing — best-effort by design.
        let mac = macro_from("quote!(struct S;)");
        let mut rec = CallRecorder::default();
        walk_macro_tokens(&mut rec, &mac);
        assert!(rec.calls.is_empty());
    }
}

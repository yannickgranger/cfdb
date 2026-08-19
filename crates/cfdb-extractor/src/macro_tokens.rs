use syn::visit::Visit;

pub(crate) fn walk_macro_tokens<V>(visitor: &mut V, mac: &syn::Macro)
where
    V: for<'ast> Visit<'ast>,
{
    use syn::parse::Parser;
    let tokens = mac.tokens.clone();

    let punct_parser = syn::punctuated::Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated;
    if let Ok(exprs) = punct_parser.parse2(tokens.clone()) {
        for expr in &exprs {
            visitor.visit_expr(expr);
        }
        return;
    }
    if let Ok(block) = syn::parse2::<syn::Block>(tokens.clone()) {
        visitor.visit_block(&block);
        return;
    }
    if let Ok(expr) = syn::parse2::<syn::Expr>(tokens) {
        visitor.visit_expr(&expr);
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use syn::visit::Visit;

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

    fn macro_from(src: &str) -> syn::Macro {
        match syn::parse_str::<syn::Expr>(src).expect("valid macro invocation expression") {
            syn::Expr::Macro(m) => m.mac,
            other => panic!("expected Expr::Macro, got {other:?}"),
        }
    }

    #[test]
    fn punctuated_expr_body_is_walked() {
        let mac = macro_from("vec![a(), b.c()]");
        let mut rec = CallRecorder::default();
        walk_macro_tokens(&mut rec, &mac);
        assert!(rec.calls.contains(&"a".to_string()));
        assert!(rec.calls.contains(&"c".to_string()));
    }

    #[test]
    fn single_expression_body_is_walked() {
        let mac = macro_from("dbg!(foo())");
        let mut rec = CallRecorder::default();
        walk_macro_tokens(&mut rec, &mac);
        assert!(rec.calls.contains(&"foo".to_string()));
    }

    #[test]
    fn unparseable_body_is_silent() {
        let mac = macro_from("quote!(struct S;)");
        let mut rec = CallRecorder::default();
        walk_macro_tokens(&mut rec, &mac);
        assert!(rec.calls.is_empty());
    }
}

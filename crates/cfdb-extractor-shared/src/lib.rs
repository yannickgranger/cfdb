//! Shared helpers for syn-based argument classification.
//!
//! `syn` types cannot enter `cfdb-core`, so the shared `kind` classifier
//! lives here. Both `cfdb-extractor` and `cfdb-hir-extractor` depend on
//! this crate.

/// Coarse syntactic classification of a `syn::Expr` into the closed-set
/// `kind` string used on `:Argument` nodes.
///
/// Returns one of: `"path"`, `"method_call"`, `"call"`, `"ref"`,
/// `"literal"`, `"other"`.
///
/// Cypher ban-rules MUST NOT filter on `kind="other"` — it signals honest
/// extractor ignorance (an `Expr` variant the classifier does not model),
/// not a domain concept.
#[must_use]
pub fn classify_arg_kind(expr: &syn::Expr) -> &'static str {
    match expr {
        syn::Expr::Path(_) => "path",
        syn::Expr::MethodCall(_) => "method_call",
        syn::Expr::Call(_) => "call",
        syn::Expr::Reference(_) => "ref",
        syn::Expr::Lit(_) => "literal",
        _ => "other",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_path_expr() {
        let expr: syn::Expr = syn::parse_str("conn").unwrap();
        assert_eq!(classify_arg_kind(&expr), "path");
    }

    #[test]
    fn classify_method_call_expr() {
        let expr: syn::Expr = syn::parse_str("conn.clone()").unwrap();
        assert_eq!(classify_arg_kind(&expr), "method_call");
    }

    #[test]
    fn classify_call_expr() {
        let expr: syn::Expr = syn::parse_str("RedisInventory::new(x)").unwrap();
        assert_eq!(classify_arg_kind(&expr), "call");
    }

    #[test]
    fn classify_ref_expr() {
        let expr: syn::Expr = syn::parse_str("&self.conn").unwrap();
        assert_eq!(classify_arg_kind(&expr), "ref");
    }

    #[test]
    fn classify_literal_expr() {
        let expr: syn::Expr = syn::parse_str("42").unwrap();
        assert_eq!(classify_arg_kind(&expr), "literal");
    }

    #[test]
    fn classify_other_expr() {
        let expr: syn::Expr = syn::parse_str("a + b").unwrap();
        assert_eq!(classify_arg_kind(&expr), "other");
    }
}

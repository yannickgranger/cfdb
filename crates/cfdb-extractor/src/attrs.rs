//! Attribute extraction helpers — narrow probes that pull a single
//! string-shaped piece of information out of a `syn::Attribute` slice.
//! Each helper is a pure function; none of them touch the `Emitter`.

/// Extract the callback path string from `#[serde(default = "Utc::now")]`
/// or similar serde default-via-function attributes on a field.
///
/// Serde accepts three default shapes:
///   1. `#[serde(default)]` — uses `T::default()`, no callback to track
///   2. `#[serde(default = "path::to::fn")]` — the string-literal form
///   3. `#[serde(default, rename_all = "camelCase")]` — nested meta list
///
/// Only shape (2) carries a callback path that the cfdb extractor can
/// project into a CallSite. The returned string is the literal path value
/// the author wrote (e.g. `"Utc::now"`, `"chrono::Utc::now"`,
/// `"my_module::default_now"`).
pub(crate) fn extract_serde_default_attr(attrs: &[syn::Attribute]) -> Option<String> {
    for attr in attrs {
        if !attr.path().is_ident("serde") {
            continue;
        }
        let mut found: Option<String> = None;
        let _ = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("default") {
                if let Ok(value) = meta.value() {
                    if let Ok(lit) = value.parse::<syn::LitStr>() {
                        found = Some(lit.value());
                    }
                }
            }
            Ok(())
        });
        if found.is_some() {
            return found;
        }
    }
    None
}

/// Extract the string value of a `#[path = "..."]` attribute, if present.
pub(crate) fn extract_path_attr(attrs: &[syn::Attribute]) -> Option<String> {
    for attr in attrs {
        if !attr.path().is_ident("path") {
            continue;
        }
        // `#[path = "foo.rs"]` — NameValue syntax.
        if let syn::Meta::NameValue(nv) = &attr.meta {
            if let syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(lit_str),
                ..
            }) = &nv.value
            {
                return Some(lit_str.value());
            }
        }
    }
    None
}

/// Return true if any of the given attributes is `#[cfg(test)]` or
/// `#[cfg(all(test, ...))]` — the two canonical forms for gating a module
/// on the test build profile. We deliberately don't try to handle
/// `#[cfg(any(test, ...))]` because it's rare in practice and ambiguous:
/// the module is *sometimes* test code. Being conservative here means
/// prod-only filter rules keep working; false negatives are reported as
/// test findings rather than missed prod findings.
pub(crate) fn attrs_contain_cfg_test(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        if !attr.path().is_ident("cfg") {
            return false;
        }
        let mut has_test = false;
        let _ = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("test") {
                has_test = true;
                return Ok(());
            }
            if meta.path.is_ident("all") {
                let _ = meta.parse_nested_meta(|inner| {
                    if inner.path.is_ident("test") {
                        has_test = true;
                    }
                    Ok(())
                });
            }
            Ok(())
        });
        has_test
    })
}

/// Return true if any of the given attributes is the bare `#[test]` function
/// attribute. This complements [`attrs_contain_cfg_test`] (which checks
/// `#[cfg(test)]` on modules) by recognising free / impl fns that are test
/// functions without being nested inside a `#[cfg(test)]` module —
/// council-cfdb-wiring §B.1.1.
///
/// Only the bare `#[test]` single-ident attribute is matched. Multi-segment
/// paths like `#[tokio::test]` or `#[test_log::test]` are deliberately not
/// matched because they are custom test harnesses outside libtest, and the
/// council decision scopes this predicate to libtest-native tests only.
pub(crate) fn attrs_contain_hash_test(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        if !attr.path().is_ident("test") {
            return false;
        }
        // Bare `#[test]` parses as `Meta::Path`; `#[test(arg)]` parses as
        // `Meta::List` (not a libtest form but we reject it for safety).
        matches!(attr.meta, syn::Meta::Path(_))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn::parse_quote;

    #[test]
    fn attrs_contain_hash_test_matches_bare_test_attribute() {
        let item: syn::ItemFn = parse_quote! {
            #[test]
            fn bare_test() {}
        };
        assert!(attrs_contain_hash_test(&item.attrs));
    }

    #[test]
    fn attrs_contain_hash_test_rejects_cfg_test() {
        let item: syn::ItemFn = parse_quote! {
            #[cfg(test)]
            fn cfg_test_fn() {}
        };
        assert!(!attrs_contain_hash_test(&item.attrs));
    }

    #[test]
    fn attrs_contain_hash_test_rejects_multi_segment_tokio_test() {
        let item: syn::ItemFn = parse_quote! {
            #[tokio::test]
            fn async_test() {}
        };
        assert!(!attrs_contain_hash_test(&item.attrs));
    }

    #[test]
    fn attrs_contain_hash_test_rejects_no_attrs() {
        let item: syn::ItemFn = parse_quote! {
            fn plain() {}
        };
        assert!(!attrs_contain_hash_test(&item.attrs));
    }
}

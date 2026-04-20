//! Attribute extraction helpers — narrow probes that pull a single
//! string-shaped piece of information out of a `syn::Attribute` slice.
//! Each helper is a pure function; none of them touch the `Emitter`.

use cfdb_core::CfgGate;
use syn::punctuated::Punctuated;
use syn::Token;

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

/// Extract deprecation state from an item's attribute list (#106 /
/// RFC addendum §A2.2 row 3).
///
/// Returns `(is_deprecated, deprecation_since)`:
///
/// - `is_deprecated` is `true` when at least one `#[deprecated]` or
///   `#[deprecated(...)]` attribute appears on the item. All three
///   accepted forms trigger a match:
///   `#[deprecated]`, `#[deprecated(note = "...")]`,
///   `#[deprecated(since = "X.Y.Z", note = "...")]`.
/// - `deprecation_since` is `Some(version_string)` when the attribute
///   carries an explicit `since = "..."` key; `None` otherwise. The
///   string is the literal author-supplied value — no SemVer parsing.
///
/// Only the bare single-segment `deprecated` path is recognised.
/// Multi-segment paths like `#[serde(deprecated = "true")]` are NOT
/// treated as Rust stability deprecation (the `#[serde(...)]` namespace
/// carries unrelated semantics). This matches the discipline used by
/// [`attrs_contain_hash_test`] for `#[test]` vs `#[tokio::test]`.
///
/// Per the #43 council DDD + rust-systems verdicts, this is an
/// extractor-time fact tagged `Provenance::Extractor` — the AST walker
/// already visits item attributes, so extraction is the right layer.
/// The [`cfdb_core::enrich::EnrichBackend::enrich_deprecation`] trait
/// method exists for surface symmetry; its `PetgraphStore` override
/// returns a `ran: true, attrs_written: 0` no-op naming the extractor
/// as the real source.
pub(crate) fn extract_deprecated_attr(attrs: &[syn::Attribute]) -> (bool, Option<String>) {
    let mut is_deprecated = false;
    let mut since: Option<String> = None;
    for attr in attrs {
        if !attr.path().is_ident("deprecated") {
            continue;
        }
        is_deprecated = true;
        // `#[deprecated]` parses as `Meta::Path` — carries no kv args.
        // `#[deprecated(...)]` parses as `Meta::List` — delegate to a
        // helper so `extract_deprecated_attr`'s nesting stays flat
        // (cognitive complexity threshold per cfdb quality gates).
        if let syn::Meta::List(list) = &attr.meta {
            if let Some(v) = parse_deprecated_since(list) {
                since = Some(v);
            }
        }
        // Continue scanning in case multiple `#[deprecated]` attrs
        // exist (not a standard pattern, but harmless — last `since`
        // wins, matching Rust's own precedence).
    }
    (is_deprecated, since)
}

/// Pull the `since = "X.Y.Z"` string literal out of a
/// `#[deprecated(since = "...", note = "...")]` meta list, if present.
/// Factored out of [`extract_deprecated_attr`] so that the main loop
/// body stays shallow enough to clear the cognitive-complexity gate.
/// `note = "..."` is deliberately ignored — the current extractor
/// surface only needs the version string.
fn parse_deprecated_since(list: &syn::MetaList) -> Option<String> {
    let mut since: Option<String> = None;
    let _ = list.parse_nested_meta(|meta| {
        if !meta.path.is_ident("since") {
            return Ok(());
        }
        let value = meta.value()?;
        let lit: syn::LitStr = value.parse()?;
        since = Some(lit.value());
        Ok(())
    });
    since
}

/// Extract the feature-only `cfg(...)` gate from the item's attribute list
/// (Issue #36). Recognises `cfg(feature = "x")`, `cfg(all(...))`,
/// `cfg(any(...))`, `cfg(not(...))` and nested combinations thereof.
///
/// **All-or-nothing policy.** Returns `None` when the item either (a)
/// has no `#[cfg(...)]` attributes, or (b) carries a cfg expression with
/// any non-feature leaf (e.g. `cfg(test)`, `cfg(target_os = "…")`,
/// `cfg(unix)`). A mixed capture would force every consumer to decide
/// how to interpret a partial tree — the closed vocabulary keeps
/// downstream queries unambiguous.
///
/// **Conjunction across multiple attributes.** Multiple `#[cfg(...)]`
/// attributes on the same item conjoin (Rust semantics). When an item
/// carries both `#[cfg(feature = "a")]` and `#[cfg(feature = "b")]`
/// this helper returns `All(vec![Feature("a"), Feature("b")])`.
pub(crate) fn extract_cfg_feature_gate(attrs: &[syn::Attribute]) -> Option<CfgGate> {
    let mut gates: Vec<CfgGate> = Vec::new();
    for attr in attrs {
        if !attr.path().is_ident("cfg") {
            continue;
        }
        // `#[cfg(...)]` — parse the parenthesised contents as a Meta.
        let Ok(inner) = attr.parse_args::<syn::Meta>() else {
            // Anything that doesn't parse as a Meta (malformed or
            // macro-generated) is treated as opaque — drop the whole
            // item's gate rather than guess.
            return None;
        };
        let gate = meta_to_feature_gate(&inner)?;
        gates.push(gate);
    }
    match gates.len() {
        0 => None,
        1 => Some(gates.into_iter().next().expect("len==1 so first() exists")),
        _ => Some(CfgGate::All(gates)),
    }
}

/// Translate a single `syn::Meta` node into a `CfgGate`, or `None` if the
/// tree contains any non-feature predicate.
fn meta_to_feature_gate(meta: &syn::Meta) -> Option<CfgGate> {
    match meta {
        syn::Meta::NameValue(nv) if nv.path.is_ident("feature") => {
            if let syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(s),
                ..
            }) = &nv.value
            {
                Some(CfgGate::Feature(s.value()))
            } else {
                None
            }
        }
        syn::Meta::List(list) if list.path.is_ident("all") => {
            let children = parse_meta_list_children(list)?;
            if children.is_empty() {
                None
            } else {
                Some(CfgGate::All(children))
            }
        }
        syn::Meta::List(list) if list.path.is_ident("any") => {
            let children = parse_meta_list_children(list)?;
            if children.is_empty() {
                None
            } else {
                Some(CfgGate::Any(children))
            }
        }
        syn::Meta::List(list) if list.path.is_ident("not") => {
            let mut children = parse_meta_list_children(list)?;
            if children.len() != 1 {
                None
            } else {
                Some(CfgGate::Not(Box::new(children.remove(0))))
            }
        }
        // Any other shape (cfg(test), cfg(target_os = "linux"),
        // cfg(unix), cfg(panic = "unwind"), …) — not a feature gate.
        _ => None,
    }
}

fn parse_meta_list_children(list: &syn::MetaList) -> Option<Vec<CfgGate>> {
    let metas: Punctuated<syn::Meta, Token![,]> =
        list.parse_args_with(Punctuated::parse_terminated).ok()?;
    metas.iter().map(meta_to_feature_gate).collect()
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

    // ---- extract_cfg_feature_gate (Issue #36) ----------------------------

    fn parse_attrs(src: &str) -> Vec<syn::Attribute> {
        let wrapped = format!("{src} fn dummy() {{}}");
        let item: syn::ItemFn = syn::parse_str(&wrapped).expect("test fixture parses");
        item.attrs
    }

    #[test]
    fn extract_cfg_feature_gate_none_when_no_attrs() {
        let attrs = parse_attrs("");
        assert_eq!(extract_cfg_feature_gate(&attrs), None);
    }

    #[test]
    fn extract_cfg_feature_gate_simple_feature() {
        let attrs = parse_attrs(r#"#[cfg(feature = "async")]"#);
        assert_eq!(
            extract_cfg_feature_gate(&attrs),
            Some(CfgGate::Feature("async".into()))
        );
    }

    #[test]
    fn extract_cfg_feature_gate_all_combinator() {
        let attrs = parse_attrs(r#"#[cfg(all(feature = "a", feature = "b"))]"#);
        assert_eq!(
            extract_cfg_feature_gate(&attrs),
            Some(CfgGate::All(vec![
                CfgGate::Feature("a".into()),
                CfgGate::Feature("b".into()),
            ]))
        );
    }

    #[test]
    fn extract_cfg_feature_gate_any_combinator() {
        let attrs = parse_attrs(r#"#[cfg(any(feature = "x", feature = "y"))]"#);
        assert_eq!(
            extract_cfg_feature_gate(&attrs),
            Some(CfgGate::Any(vec![
                CfgGate::Feature("x".into()),
                CfgGate::Feature("y".into()),
            ]))
        );
    }

    #[test]
    fn extract_cfg_feature_gate_not_combinator() {
        let attrs = parse_attrs(r#"#[cfg(not(feature = "legacy"))]"#);
        assert_eq!(
            extract_cfg_feature_gate(&attrs),
            Some(CfgGate::Not(Box::new(CfgGate::Feature("legacy".into()))))
        );
    }

    #[test]
    fn extract_cfg_feature_gate_nested_combinators() {
        let attrs = parse_attrs(
            r#"#[cfg(all(feature = "async", any(feature = "tokio", not(feature = "legacy"))))]"#,
        );
        assert_eq!(
            extract_cfg_feature_gate(&attrs),
            Some(CfgGate::All(vec![
                CfgGate::Feature("async".into()),
                CfgGate::Any(vec![
                    CfgGate::Feature("tokio".into()),
                    CfgGate::Not(Box::new(CfgGate::Feature("legacy".into()))),
                ]),
            ]))
        );
    }

    #[test]
    fn extract_cfg_feature_gate_multiple_attrs_conjoin() {
        // Two separate #[cfg(...)] attributes on the same item conjoin.
        let attrs = parse_attrs(
            r#"#[cfg(feature = "a")]
               #[cfg(feature = "b")]"#,
        );
        assert_eq!(
            extract_cfg_feature_gate(&attrs),
            Some(CfgGate::All(vec![
                CfgGate::Feature("a".into()),
                CfgGate::Feature("b".into()),
            ]))
        );
    }

    #[test]
    fn extract_cfg_feature_gate_non_feature_poisons_result() {
        // Pure non-feature cfg — whole item gate drops to None.
        assert_eq!(
            extract_cfg_feature_gate(&parse_attrs(r#"#[cfg(test)]"#)),
            None
        );
        assert_eq!(
            extract_cfg_feature_gate(&parse_attrs(r#"#[cfg(unix)]"#)),
            None
        );
        assert_eq!(
            extract_cfg_feature_gate(&parse_attrs(r#"#[cfg(target_os = "linux")]"#)),
            None
        );
    }

    #[test]
    fn extract_cfg_feature_gate_mixed_feature_and_non_feature_drops_to_none() {
        // Even ONE non-feature leaf poisons the whole tree (all-or-nothing).
        let attrs = parse_attrs(r#"#[cfg(all(feature = "x", target_os = "linux"))]"#);
        assert_eq!(extract_cfg_feature_gate(&attrs), None);
    }

    #[test]
    fn extract_cfg_feature_gate_ignores_non_cfg_attrs() {
        // #[derive(Debug)], #[serde(default)] etc. must not leak into
        // the gate computation.
        let attrs = parse_attrs(
            r#"#[derive(Debug)]
               #[cfg(feature = "async")]
               #[must_use]"#,
        );
        assert_eq!(
            extract_cfg_feature_gate(&attrs),
            Some(CfgGate::Feature("async".into()))
        );
    }

    // ---- extract_deprecated_attr (#106 — RFC addendum §A2.2 row 3) ------
    //
    // Extractor-time fact per #43 council DDD + rust-systems verdicts:
    // the `#[deprecated]` attribute is syntactic; the AST walker already
    // visits item attributes, so extraction is the right layer.

    #[test]
    fn extract_deprecated_attr_none_when_no_attrs() {
        let attrs = parse_attrs("");
        assert_eq!(extract_deprecated_attr(&attrs), (false, None));
    }

    #[test]
    fn extract_deprecated_attr_bare_form() {
        // `#[deprecated]` on its own — deprecated, no since version.
        let attrs = parse_attrs(r#"#[deprecated]"#);
        assert_eq!(extract_deprecated_attr(&attrs), (true, None));
    }

    #[test]
    fn extract_deprecated_attr_since_form() {
        // `#[deprecated(since = "1.2.0")]` — since version captured.
        let attrs = parse_attrs(r#"#[deprecated(since = "1.2.0")]"#);
        assert_eq!(
            extract_deprecated_attr(&attrs),
            (true, Some("1.2.0".to_string()))
        );
    }

    #[test]
    fn extract_deprecated_attr_note_only_form() {
        // `#[deprecated(note = "use Foo instead")]` — deprecated, no since.
        let attrs = parse_attrs(r#"#[deprecated(note = "use Foo instead")]"#);
        assert_eq!(extract_deprecated_attr(&attrs), (true, None));
    }

    #[test]
    fn extract_deprecated_attr_since_and_note_form() {
        let attrs =
            parse_attrs(r#"#[deprecated(since = "2.0.0", note = "legacy path; see #123")]"#);
        assert_eq!(
            extract_deprecated_attr(&attrs),
            (true, Some("2.0.0".to_string()))
        );
    }

    #[test]
    fn extract_deprecated_attr_ignores_non_deprecated_attrs() {
        // `#[deprecated]` must still dominate even when other attrs
        // surround it.
        let attrs = parse_attrs(
            r#"#[derive(Debug)]
               #[deprecated(since = "3.1.4")]
               #[must_use]"#,
        );
        assert_eq!(
            extract_deprecated_attr(&attrs),
            (true, Some("3.1.4".to_string()))
        );
    }

    #[test]
    fn extract_deprecated_attr_rejects_multi_segment_path() {
        // `#[serde::deprecated(...)]` (hypothetical) is NOT the standard
        // Rust deprecation attribute and must not be matched. Only the
        // bare `deprecated` path counts.
        let attrs = parse_attrs(r#"#[serde(deprecated = "true")]"#);
        assert_eq!(extract_deprecated_attr(&attrs), (false, None));
    }
}

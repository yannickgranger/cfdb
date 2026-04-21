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

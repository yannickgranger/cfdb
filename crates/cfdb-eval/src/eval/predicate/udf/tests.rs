use super::*;

fn normalize_signature(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_ws = false;
    for c in s.trim().chars() {
        if c.is_whitespace() {
            if !prev_ws {
                out.push(' ');
                prev_ws = true;
            }
        } else {
            out.push(c);
            prev_ws = false;
        }
    }
    out
}

mod signature_divergent_tests {
    use super::normalize_signature;

    #[test]
    fn trim_outer_whitespace() {
        assert_eq!(normalize_signature("  fn() -> ()  "), "fn() -> ()");
    }

    #[test]
    fn collapse_internal_whitespace() {
        assert_eq!(
            normalize_signature("fn(i32,   String)  ->  bool"),
            "fn(i32, String) -> bool"
        );
    }

    #[test]
    fn identical_normalized_strings_are_not_divergent() {
        let a = normalize_signature("fn(i32) -> bool");
        let b = normalize_signature("fn(i32) -> bool");
        assert_eq!(a, b);
    }

    #[test]
    fn whitespace_only_difference_is_not_divergent() {
        let a = normalize_signature("fn(i32) -> bool");
        let b = normalize_signature("fn(i32)  ->   bool");
        assert_eq!(a, b);
    }

    #[test]
    fn different_types_are_divergent() {
        let a = normalize_signature("fn() -> f64");
        let b = normalize_signature("fn() -> (f64, f64)");
        assert_ne!(a, b);
    }

    use super::signatures_differ_modulo_whitespace;

    fn equivalent_to_normalize(a: &str, b: &str) -> bool {
        let via_normalize = normalize_signature(a) != normalize_signature(b);
        let via_fast = signatures_differ_modulo_whitespace(a, b);
        assert_eq!(
            via_normalize, via_fast,
            "divergence equivalence broken: a={a:?}, b={b:?}, normalize_form={via_normalize}, fast_form={via_fast}"
        );
        via_fast
    }

    #[test]
    fn fast_form_matches_normalize_on_identical() {
        assert!(!equivalent_to_normalize(
            "fn(i32) -> bool",
            "fn(i32) -> bool"
        ));
    }

    #[test]
    fn fast_form_matches_normalize_on_outer_whitespace() {
        assert!(!equivalent_to_normalize(
            "  fn(i32) -> bool  ",
            "fn(i32) -> bool"
        ));
    }

    #[test]
    fn fast_form_matches_normalize_on_collapsed_internal_whitespace() {
        assert!(!equivalent_to_normalize(
            "fn(i32,   String)  ->  bool",
            "fn(i32, String) -> bool"
        ));
    }

    #[test]
    fn fast_form_matches_normalize_on_genuine_divergence() {
        assert!(equivalent_to_normalize("fn() -> f64", "fn() -> (f64, f64)"));
    }

    #[test]
    fn fast_form_distinguishes_present_vs_absent_whitespace_gap() {
        assert!(equivalent_to_normalize("Foo Bar", "FooBar"));
    }

    #[test]
    fn fast_form_tab_and_newline_equal_space() {
        assert!(!equivalent_to_normalize(
            "fn(i32)\t->\nbool",
            "fn(i32) -> bool"
        ));
    }

    #[test]
    fn fast_form_handles_empty_strings() {
        assert!(!equivalent_to_normalize("", ""));
        assert!(!equivalent_to_normalize("   ", ""));
        assert!(equivalent_to_normalize("", "fn()"));
    }

    #[test]
    fn fast_form_handles_unicode_signatures() {
        assert!(!equivalent_to_normalize(
            "fn(α: i32) -> β",
            "fn(α: i32) -> β"
        ));
        assert!(equivalent_to_normalize("fn(α: i32)", "fn(β: i32)"));
    }
}

mod entries_overlap_tests {
    use super::{entries_jaccard_impl, entries_subset_impl, overlap_verdict_impl};

    #[test]
    fn empty_is_subset_of_anything_str() {
        assert!(entries_subset_impl("[]", r#"["EUR","USD"]"#));
    }

    #[test]
    fn empty_is_subset_of_empty() {
        assert!(entries_subset_impl("[]", "[]"));
    }

    #[test]
    fn equal_str_sets_are_subsets_of_each_other() {
        let a = r#"["EUR","GBP","USD"]"#;
        let b = r#"["EUR","GBP","USD"]"#;
        assert!(entries_subset_impl(a, b));
        assert!(entries_subset_impl(b, a));
    }

    #[test]
    fn strict_subset_str_returns_true_one_way() {
        let small = r#"["EUR","USD"]"#;
        let big = r#"["EUR","GBP","USD"]"#;
        assert!(entries_subset_impl(small, big));
        assert!(!entries_subset_impl(big, small));
    }

    #[test]
    fn strict_subset_int_returns_true_one_way() {
        let small = "[1,2]";
        let big = "[1,2,3]";
        assert!(entries_subset_impl(small, big));
        assert!(!entries_subset_impl(big, small));
    }

    #[test]
    fn disjoint_str_sets_are_not_subsets() {
        let a = r#"["EUR","USD"]"#;
        let b = r#"["JPY","CHF"]"#;
        assert!(!entries_subset_impl(a, b));
        assert!(!entries_subset_impl(b, a));
    }

    #[test]
    fn mixed_element_type_is_not_subset_either_way() {
        let strs = r#"["1","2"]"#;
        let ints = "[1,2]";
        assert!(!entries_subset_impl(strs, ints));
        assert!(!entries_subset_impl(ints, strs));
    }

    #[test]
    fn invalid_json_is_not_subset_either_way() {
        assert!(!entries_subset_impl("not json", r#"["a"]"#));
        assert!(!entries_subset_impl(r#"["a"]"#, "not json"));
    }

    #[test]
    fn jaccard_of_two_empty_sets_is_zero() {
        assert_eq!(entries_jaccard_impl("[]", "[]"), 0.0);
    }

    #[test]
    fn jaccard_of_identical_str_sets_is_one() {
        let a = r#"["EUR","GBP","USD"]"#;
        let b = r#"["EUR","GBP","USD"]"#;
        assert_eq!(entries_jaccard_impl(a, b), 1.0);
    }

    #[test]
    fn jaccard_of_identical_int_sets_is_one() {
        assert_eq!(entries_jaccard_impl("[1,2,3]", "[1,2,3]"), 1.0);
    }

    #[test]
    fn jaccard_half_overlap_str_is_one_third() {
        let a = r#"["a","b"]"#;
        let b = r#"["b","c"]"#;
        let j = entries_jaccard_impl(a, b);
        assert!((j - (1.0 / 3.0)).abs() < 1e-12, "got {j}");
    }

    #[test]
    fn jaccard_half_overlap_str_at_threshold() {
        let a = r#"["a","b","c"]"#;
        let b = r#"["b","c","d"]"#;
        let j = entries_jaccard_impl(a, b);
        assert!((j - 0.5).abs() < 1e-12, "got {j}");
        assert!(j >= 0.5);
    }

    #[test]
    fn jaccard_disjoint_sets_is_zero() {
        let a = r#"["EUR","USD"]"#;
        let b = r#"["JPY","CHF"]"#;
        assert_eq!(entries_jaccard_impl(a, b), 0.0);
    }

    #[test]
    fn jaccard_subset_int_is_ratio_of_sizes() {
        let j = entries_jaccard_impl("[1,2]", "[1,2,3,4]");
        assert!((j - 0.5).abs() < 1e-12, "got {j}");
    }

    #[test]
    fn jaccard_mixed_element_types_is_zero() {
        let strs = r#"["1","2"]"#;
        let ints = "[1,2]";
        assert_eq!(entries_jaccard_impl(strs, ints), 0.0);
        assert_eq!(entries_jaccard_impl(ints, strs), 0.0);
    }

    #[test]
    fn jaccard_invalid_json_is_zero() {
        assert_eq!(entries_jaccard_impl("not json", r#"["a"]"#), 0.0);
        assert_eq!(entries_jaccard_impl(r#"["a"]"#, "not json"), 0.0);
    }

    #[test]
    fn jaccard_empty_vs_populated_is_zero() {
        assert_eq!(entries_jaccard_impl("[]", r#"["a","b"]"#), 0.0);
        assert_eq!(entries_jaccard_impl(r#"["a","b"]"#, "[]"), 0.0);
    }

    #[test]
    fn overlap_verdict_duplicate_when_hashes_equal() {
        let v = overlap_verdict_impl(r#"["a"]"#, r#"["a"]"#, "deadbeef", "deadbeef");
        assert_eq!(v, "CONST_TABLE_DUPLICATE");
    }

    #[test]
    fn overlap_verdict_subset_when_strict_subset_and_hashes_differ() {
        let v = overlap_verdict_impl(r#"["a","b"]"#, r#"["a","b","c"]"#, "h_small", "h_big");
        assert_eq!(v, "CONST_TABLE_SUBSET");
    }

    #[test]
    fn overlap_verdict_subset_in_either_order() {
        let v = overlap_verdict_impl(r#"["a","b","c"]"#, r#"["a","b"]"#, "h_big", "h_small");
        assert_eq!(v, "CONST_TABLE_SUBSET");
    }

    #[test]
    fn overlap_verdict_intersection_high_when_jaccard_at_threshold() {
        let v = overlap_verdict_impl(r#"["a","b","c"]"#, r#"["b","c","d"]"#, "h_left", "h_right");
        assert_eq!(v, "CONST_TABLE_INTERSECTION_HIGH");
    }

    #[test]
    fn overlap_verdict_none_when_jaccard_below_threshold() {
        let v = overlap_verdict_impl(
            r#"["a","b","c","d"]"#,
            r#"["c","e","f","g"]"#,
            "h_left",
            "h_right",
        );
        assert_eq!(v, "CONST_TABLE_NONE");
    }

    #[test]
    fn overlap_verdict_none_when_disjoint() {
        let v = overlap_verdict_impl(r#"["EUR","USD"]"#, r#"["JPY","CHF"]"#, "h_left", "h_right");
        assert_eq!(v, "CONST_TABLE_NONE");
    }
}

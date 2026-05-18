//! Helpers shared by the `signature_divergent` / `entries_subset` /
//! `entries_jaccard` / `overlap_verdict` Cypher UDFs evaluated in
//! [`super::Evaluator`]. The pure-function impls live here so the
//! enclosing `predicate.rs` stays focused on the visitor + dispatch
//! surface; their unit tests live alongside them (rather than reaching
//! across the file split) so a future change to a helper and its test
//! is a one-file diff.

/// Parsed JSON-array element set for the RFC-040 §3.4 overlap UDFs.
///
/// `entries_normalized` is JSON-array-as-string of either all strings
/// (`["a","b"]`) or all numbers (`[1,2]`) — the element type is
/// inferred from the first parseable element. Mixed-element-type
/// inputs (e.g. `["a", 1]`) are forbidden by the wire contract; the
/// UDFs treat them as `MixedOrInvalid` so the enclosing rule sees no
/// overlap (RFC-040 §3.4 N2).
#[derive(Debug, PartialEq, Eq)]
enum NormalizedEntries {
    Strs(std::collections::BTreeSet<String>),
    Ints(std::collections::BTreeSet<i64>),
    /// Empty input (`[]`) — distinct from MixedOrInvalid because empty
    /// is a valid set (subset of anything; jaccard 0/0 → 0.0). The
    /// element type is unknown but operations against another empty
    /// or any populated set are well-defined.
    Empty,
    /// Either a parse error or a mixed-element-type input. Both UDFs
    /// treat this as "no overlap" rather than propagating a parse
    /// failure, because the wire contract guarantees well-formed
    /// `entries_normalized`; a malformed value is best surfaced as
    /// "no row matches" in the rule rather than an evaluator panic.
    MixedOrInvalid,
}

/// Parse the `entries_normalized` JSON-array string into a sorted set
/// suitable for set-relationship comparison. Element type is inferred
/// from the first element; mixed-type or invalid input collapses to
/// [`NormalizedEntries::MixedOrInvalid`].
fn parse_entries_normalized(s: &str) -> NormalizedEntries {
    let parsed: serde_json::Value = match serde_json::from_str(s) {
        Ok(v) => v,
        Err(_) => return NormalizedEntries::MixedOrInvalid,
    };
    let serde_json::Value::Array(items) = parsed else {
        return NormalizedEntries::MixedOrInvalid;
    };
    if items.is_empty() {
        return NormalizedEntries::Empty;
    }
    // Infer element type from the first element. Number elements are
    // matched as i64 because the extractor emits decimal-stringified
    // integers per RFC-040 §3.4; non-integral floats are not in the
    // wire-shape vocabulary and collapse to MixedOrInvalid.
    //
    // Decide kind via a snapshot of the first element (borrow ends at
    // the matches!), then consume `items` by-value in each branch — the
    // owned String / Number variants move into the result set with no
    // clone, dropping the loop-clone flagged by quality-metrics.
    let first_is_string = matches!(&items[0], serde_json::Value::String(_));
    let first_is_number = matches!(&items[0], serde_json::Value::Number(_));
    if first_is_string {
        let mut set = std::collections::BTreeSet::new();
        for v in items {
            let serde_json::Value::String(s) = v else {
                return NormalizedEntries::MixedOrInvalid;
            };
            set.insert(s);
        }
        NormalizedEntries::Strs(set)
    } else if first_is_number {
        let mut set = std::collections::BTreeSet::new();
        for v in items {
            let serde_json::Value::Number(n) = v else {
                return NormalizedEntries::MixedOrInvalid;
            };
            let Some(i) = n.as_i64() else {
                return NormalizedEntries::MixedOrInvalid;
            };
            set.insert(i);
        }
        NormalizedEntries::Ints(set)
    } else {
        NormalizedEntries::MixedOrInvalid
    }
}

/// `entries_subset` impl — true iff every element of `a` is in `b`.
/// Empty is a subset of anything; equal sets are subsets of each
/// other. Cross-element-type or invalid inputs return `false`.
pub(super) fn entries_subset_impl(a_json: &str, b_json: &str) -> bool {
    let a = parse_entries_normalized(a_json);
    let b = parse_entries_normalized(b_json);
    match (a, b) {
        (NormalizedEntries::Empty, _) => true,
        (NormalizedEntries::MixedOrInvalid, _) | (_, NormalizedEntries::MixedOrInvalid) => false,
        (NormalizedEntries::Strs(sa), NormalizedEntries::Strs(sb)) => sa.is_subset(&sb),
        (NormalizedEntries::Ints(ia), NormalizedEntries::Ints(ib)) => ia.is_subset(&ib),
        // Cross-element-type — no overlap by RFC-040 §3.4 N2. The
        // empty-on-the-right case (e.g. Strs vs Empty) is `false`
        // because a populated set is never a subset of empty; the
        // empty-on-the-left case is handled by the first arm above.
        (NormalizedEntries::Strs(_), NormalizedEntries::Ints(_))
        | (NormalizedEntries::Ints(_), NormalizedEntries::Strs(_))
        | (NormalizedEntries::Strs(_), NormalizedEntries::Empty)
        | (NormalizedEntries::Ints(_), NormalizedEntries::Empty) => false,
    }
}

/// `entries_jaccard` impl — `|a ∩ b| / |a ∪ b|`.
/// Returns `0.0` if both inputs are empty (avoid divide-by-zero) and
/// `0.0` for cross-element-type / invalid input.
pub(super) fn entries_jaccard_impl(a_json: &str, b_json: &str) -> f64 {
    let a = parse_entries_normalized(a_json);
    let b = parse_entries_normalized(b_json);
    match (a, b) {
        (NormalizedEntries::Empty, NormalizedEntries::Empty) => 0.0,
        (NormalizedEntries::MixedOrInvalid, _) | (_, NormalizedEntries::MixedOrInvalid) => 0.0,
        (NormalizedEntries::Strs(sa), NormalizedEntries::Strs(sb)) => jaccard_btree(&sa, &sb),
        (NormalizedEntries::Ints(ia), NormalizedEntries::Ints(ib)) => jaccard_btree(&ia, &ib),
        // Cross-element-type or empty-vs-populated — no overlap. The
        // empty-vs-populated case returns 0.0 because |∩| = 0, |∪| =
        // |populated|, ratio is 0.0.
        (NormalizedEntries::Strs(_), NormalizedEntries::Ints(_))
        | (NormalizedEntries::Ints(_), NormalizedEntries::Strs(_))
        | (NormalizedEntries::Empty, NormalizedEntries::Strs(_))
        | (NormalizedEntries::Empty, NormalizedEntries::Ints(_))
        | (NormalizedEntries::Strs(_), NormalizedEntries::Empty)
        | (NormalizedEntries::Ints(_), NormalizedEntries::Empty) => 0.0,
    }
}

fn jaccard_btree<T: Ord>(
    a: &std::collections::BTreeSet<T>,
    b: &std::collections::BTreeSet<T>,
) -> f64 {
    let inter = a.intersection(b).count() as f64;
    let union = a.union(b).count() as f64;
    if union == 0.0 {
        0.0
    } else {
        inter / union
    }
}

/// `overlap_verdict` impl — RFC-040 §3.4 precedence-decoder.
/// See [`Evaluator::call_overlap_verdict`] for the contract.
pub(super) fn overlap_verdict_impl(
    a_norm: &str,
    b_norm: &str,
    a_hash: &str,
    b_hash: &str,
) -> &'static str {
    if a_hash == b_hash {
        return "CONST_TABLE_DUPLICATE";
    }
    if entries_subset_impl(a_norm, b_norm) || entries_subset_impl(b_norm, a_norm) {
        return "CONST_TABLE_SUBSET";
    }
    if entries_jaccard_impl(a_norm, b_norm) >= 0.5 {
        return "CONST_TABLE_INTERSECTION_HIGH";
    }
    "CONST_TABLE_NONE"
}

/// Normalize a signature string for `signature_divergent` comparison —
/// trim outer whitespace and collapse any run of internal whitespace to
/// a single ASCII space. See [`super::Evaluator::call_signature_divergent`]
/// for the rationale.
///
/// **Test-only reference form.** The production UDF dispatches
/// through [`signatures_differ_modulo_whitespace`] (equivalent
/// semantics, zero allocation, #409). The normalize form is retained
/// for the unit-test corpus that locks the normalization shape AND
/// for the equivalence assertion in `signature_divergent_tests`.
#[cfg(test)]
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

/// `signatures_differ_modulo_whitespace(a, b)` — equivalent to
/// `normalize_signature(a) != normalize_signature(b)` but iterates
/// both inputs once with zero heap allocation. Used by
/// [`super::Evaluator::call_signature_divergent`] on the
/// `signature-divergent.cypher` hot path (#409). Pre-fix wall-time
/// on cfdb-self smoke was 542s for that one query; the double
/// allocation per invocation was the dominant cost.
///
/// # Semantics (locked by `signature_divergent_tests`)
///
/// Two signatures are equivalent (returns `false` = not divergent)
/// iff their non-whitespace characters are identical in order AND
/// the presence/absence of whitespace between each adjacent pair of
/// non-whitespace characters agrees. Outer whitespace is ignored.
///
/// # Algorithm
///
/// The `normalize_signature` form trims outer whitespace and
/// collapses every internal whitespace run to a single space. Two
/// such normalized strings are equal iff:
///
/// 1. Their non-whitespace characters are identical in order, AND
/// 2. Between each adjacent pair of non-whitespace characters, both
///    have either zero whitespace or some non-zero amount.
///
/// We decide both in one fused walk by stepping pairwise through
/// `a.chars()` and `b.chars()` after `trim()`. Track each side's
/// "had-whitespace-since-prev" state via [`skip_ws_run`]; at each
/// non-whitespace step require both the character to match and the
/// had-ws flags to agree.
pub(super) fn signatures_differ_modulo_whitespace(a: &str, b: &str) -> bool {
    let mut ai = a.trim().chars().peekable();
    let mut bi = b.trim().chars().peekable();
    loop {
        let (a_ws, a_next) = skip_ws_run(&mut ai);
        let (b_ws, b_next) = skip_ws_run(&mut bi);
        // Disagreement on whether a whitespace gap appears here =
        // divergent (one has `Foo Bar`, the other has `FooBar`).
        if a_ws != b_ws {
            return true;
        }
        match (a_next, b_next) {
            (None, None) => return false,
            (Some(ca), Some(cb)) if ca == cb => {
                ai.next();
                bi.next();
            }
            _ => return true,
        }
    }
}

/// Advance `iter` past a run of whitespace characters; return whether
/// any whitespace was consumed and a peek at the next non-whitespace
/// character (or `None` at end of input). Helper for
/// [`signatures_differ_modulo_whitespace`].
fn skip_ws_run<I: Iterator<Item = char>>(
    iter: &mut std::iter::Peekable<I>,
) -> (bool, Option<char>) {
    let mut saw_ws = false;
    while let Some(&c) = iter.peek() {
        if c.is_whitespace() {
            iter.next();
            saw_ws = true;
        } else {
            return (saw_ws, Some(c));
        }
    }
    (saw_ws, None)
}

#[cfg(test)]
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

    // ---- signatures_differ_modulo_whitespace (#409 hot-path fast path) ----
    //
    // The runtime UDF dispatches through this fn, not through the
    // `normalize_signature` allocating form. The contract: for every
    // (a, b) pair, `signatures_differ_modulo_whitespace(a, b) ==
    // (normalize_signature(a) != normalize_signature(b))`. We lock
    // that equivalence on a corpus that exercises every whitespace
    // shape the extractor can emit today.
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
        // `Foo Bar` vs `FooBar` — normalize collapses one space, the
        // other has none, so they differ after normalize → divergent.
        assert!(equivalent_to_normalize("Foo Bar", "FooBar"));
    }

    #[test]
    fn fast_form_tab_and_newline_equal_space() {
        // The normalize form treats any is_whitespace() char as
        // collapsible whitespace; the fast form must too.
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
        // The normalize form uses `chars()` (Unicode scalars); the
        // fast form must too.
        assert!(!equivalent_to_normalize(
            "fn(α: i32) -> β",
            "fn(α: i32) -> β"
        ));
        assert!(equivalent_to_normalize("fn(α: i32)", "fn(β: i32)"));
    }
}

#[cfg(test)]
mod entries_overlap_tests {
    //! Unit tests for the RFC-040 §3.4 overlap UDFs
    //! (`entries_subset`, `entries_jaccard`, `overlap_verdict`).
    //!
    //! Pure-function impls (`entries_subset_impl`, `entries_jaccard_impl`,
    //! `overlap_verdict_impl`) are exercised directly so the test surface
    //! is independent of the dispatch wrapper. Dispatch wiring is
    //! covered by the integration scar in
    //! `crates/cfdb-cli/tests/const_table_overlap.rs`.
    use super::{entries_jaccard_impl, entries_subset_impl, overlap_verdict_impl};

    // ---- entries_subset --------------------------------------------------

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
        // ["EUR","USD"] ⊂ ["EUR","GBP","USD"]
        let small = r#"["EUR","USD"]"#;
        let big = r#"["EUR","GBP","USD"]"#;
        assert!(entries_subset_impl(small, big));
        // superset is NOT a subset of the smaller set
        assert!(!entries_subset_impl(big, small));
    }

    #[test]
    fn strict_subset_int_returns_true_one_way() {
        // [1,2] ⊂ [1,2,3]
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
        // RFC-040 §3.4 N2 — mixed-type inputs return false.
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

    // ---- entries_jaccard -------------------------------------------------

    #[test]
    fn jaccard_of_two_empty_sets_is_zero() {
        // RFC-040 §3.4 — divide-by-zero guard.
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
        // {a,b} vs {b,c} → |∩|=1, |∪|=3, ratio = 1/3.
        let a = r#"["a","b"]"#;
        let b = r#"["b","c"]"#;
        let j = entries_jaccard_impl(a, b);
        assert!((j - (1.0 / 3.0)).abs() < 1e-12, "got {j}");
    }

    #[test]
    fn jaccard_half_overlap_str_at_threshold() {
        // {a,b,c} vs {b,c,d} → |∩|=2, |∪|=4, ratio = 0.5 (the RFC §3.4
        // INTERSECTION_HIGH threshold). Pin the boundary value
        // explicitly so a future refactor cannot drift it across 0.5.
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
        // [1,2] ⊂ [1,2,3,4] — |∩|=2, |∪|=4, ratio = 0.5.
        let j = entries_jaccard_impl("[1,2]", "[1,2,3,4]");
        assert!((j - 0.5).abs() < 1e-12, "got {j}");
    }

    #[test]
    fn jaccard_mixed_element_types_is_zero() {
        // RFC-040 §3.4 N2 — mixed-type inputs return 0.0.
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
        // Empty vs populated: |∩|=0, |∪|=|populated|, ratio = 0.0.
        assert_eq!(entries_jaccard_impl("[]", r#"["a","b"]"#), 0.0);
        assert_eq!(entries_jaccard_impl(r#"["a","b"]"#, "[]"), 0.0);
    }

    // ---- overlap_verdict precedence -------------------------------------

    #[test]
    fn overlap_verdict_duplicate_when_hashes_equal() {
        // hash equality is the canonical set-equality key (RFC-040 §3.1) —
        // takes precedence over subset / jaccard regardless of normalized
        // contents.
        let v = overlap_verdict_impl(r#"["a"]"#, r#"["a"]"#, "deadbeef", "deadbeef");
        assert_eq!(v, "CONST_TABLE_DUPLICATE");
    }

    #[test]
    fn overlap_verdict_subset_when_strict_subset_and_hashes_differ() {
        // Strict subset — different hashes (different sizes), one is a
        // subset of the other.
        let v = overlap_verdict_impl(r#"["a","b"]"#, r#"["a","b","c"]"#, "h_small", "h_big");
        assert_eq!(v, "CONST_TABLE_SUBSET");
    }

    #[test]
    fn overlap_verdict_subset_in_either_order() {
        // a ⊃ b is also CONST_TABLE_SUBSET — the rule is symmetric on the
        // pair; the verdict fires when either side is a subset of the
        // other.
        let v = overlap_verdict_impl(r#"["a","b","c"]"#, r#"["a","b"]"#, "h_big", "h_small");
        assert_eq!(v, "CONST_TABLE_SUBSET");
    }

    #[test]
    fn overlap_verdict_intersection_high_when_jaccard_at_threshold() {
        // {a,b,c} vs {b,c,d} — jaccard 0.5, neither is a subset of the
        // other. RFC-040 §3.4 third-tier verdict.
        let v = overlap_verdict_impl(r#"["a","b","c"]"#, r#"["b","c","d"]"#, "h_left", "h_right");
        assert_eq!(v, "CONST_TABLE_INTERSECTION_HIGH");
    }

    #[test]
    fn overlap_verdict_none_when_jaccard_below_threshold() {
        // {a,b,c,d} vs {c,e,f,g} — jaccard 1/7 ≈ 0.143, no subset
        // relation, no hash match → NONE.
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

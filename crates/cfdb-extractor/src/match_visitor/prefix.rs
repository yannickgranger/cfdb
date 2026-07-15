//! Pure prefix-extraction over `match`-arm patterns (RFC-053 §3.1).
//!
//! The functions here take `syn` AST fragments and return values — zero
//! I/O, zero emission. [`match_facts`] is the entry point the visitor
//! calls once per `match` expression; the recursion and the wildcard
//! heuristic below are its building blocks and are unit-tested directly.

use std::collections::BTreeSet;

/// The per-`match`-expression facts extracted from its arm list: the
/// distinct name-level matched-path prefixes (RFC-053 §3.1), the arm
/// count, and the wildcard flag. One `:MatchSite` is emitted per prefix.
pub(crate) struct MatchFacts {
    /// Distinct matched-path prefixes in sorted order — deduped BEFORE the
    /// visitor assigns occurrence-counter ids (RFC-053 §3.1
    /// dedup-before-id), so a multi-arm single-prefix `match` yields
    /// exactly one entry.
    pub(crate) prefixes: Vec<String>,
    pub(crate) arm_count: u32,
    pub(crate) wildcard: bool,
}

/// Extract the [`MatchFacts`] for one `match` expression from its arm
/// list. Pure over the arms — the dedup (`BTreeSet`) happens here, so the
/// caller sees each prefix once regardless of how many arms carry it.
pub(crate) fn match_facts(arms: &[syn::Arm]) -> MatchFacts {
    let mut prefixes: BTreeSet<String> = BTreeSet::new();
    let mut wildcard = false;
    for arm in arms {
        collect_pattern_prefixes(&arm.pat, &mut prefixes);
        if is_wildcard_arm(&arm.pat) {
            wildcard = true;
        }
    }
    MatchFacts {
        prefixes: prefixes.into_iter().collect(),
        arm_count: arms.len() as u32,
        wildcard,
    }
}

/// Walk one arm pattern recursively through the closed `syn::Pat` variant
/// list (RFC-053 §3.1) and insert the all-but-last-segment prefix of every
/// multi-segment path it carries. `syn::Pat` is `#[non_exhaustive]`; every
/// currently-known variant is handled explicitly and the trailing `_` arm
/// covers only future variants (they fall through to no contribution — the
/// future-variant panic worry is moot).
fn collect_pattern_prefixes(pat: &syn::Pat, out: &mut BTreeSet<String>) {
    match pat {
        // Path-bearing — contribute a prefix, and (for the two container
        // forms) recurse into their sub-patterns.
        syn::Pat::Path(p) => push_path_prefix(&p.path, out),
        syn::Pat::TupleStruct(ts) => {
            push_path_prefix(&ts.path, out);
            for elem in &ts.elems {
                collect_pattern_prefixes(elem, out);
            }
        }
        syn::Pat::Struct(s) => {
            push_path_prefix(&s.path, out);
            for field in &s.fields {
                collect_pattern_prefixes(&field.pat, out);
            }
        }
        // Containers — no own path; recurse.
        syn::Pat::Ident(pi) => {
            if let Some((_, subpat)) = &pi.subpat {
                collect_pattern_prefixes(subpat, out);
            }
        }
        syn::Pat::Reference(r) => collect_pattern_prefixes(&r.pat, out),
        syn::Pat::Or(o) => {
            for case in &o.cases {
                collect_pattern_prefixes(case, out);
            }
        }
        syn::Pat::Paren(p) => collect_pattern_prefixes(&p.pat, out),
        syn::Pat::Tuple(t) => {
            for elem in &t.elems {
                collect_pattern_prefixes(elem, out);
            }
        }
        syn::Pat::Slice(s) => {
            for elem in &s.elems {
                collect_pattern_prefixes(elem, out);
            }
        }
        // Leaves — contribute no path (RFC-053 §3.1 closed variant list).
        syn::Pat::Wild(_)
        | syn::Pat::Rest(_)
        | syn::Pat::Lit(_)
        | syn::Pat::Range(_)
        | syn::Pat::Const(_)
        | syn::Pat::Macro(_)
        | syn::Pat::Verbatim(_)
        | syn::Pat::Type(_) => {}
        // Future `#[non_exhaustive]` variants — no contribution.
        _ => {}
    }
}

/// Insert the all-but-last-segment prefix of `path` when it has ≥ 2
/// segments. Single-segment paths (and bare idents, handled as
/// `Pat::Ident` above) are indistinguishable from bindings at the syn
/// level and are skipped — RFC-053 §3.1 named recall limit #1. Segment
/// idents are joined with `::`, ignoring generic arguments, mirroring
/// `type_render::render_path`.
fn push_path_prefix(path: &syn::Path, out: &mut BTreeSet<String>) {
    let segments: Vec<String> = path.segments.iter().map(|s| s.ident.to_string()).collect();
    if segments.len() >= 2 {
        out.insert(segments[..segments.len() - 1].join("::"));
    }
}

/// True iff `pat` (an arm's top-level pattern) is a wildcard arm per the
/// RFC-053 §3.1 heuristic: a top-level `_` (`Pat::Wild`) OR a bare
/// `Pat::Ident` with no sub-pattern whose identifier starts lowercase (a
/// fresh binding, not a unit-variant/const path — syn does no name
/// resolution, so this is a documented heuristic, named recall limit #2).
/// Covers BOTH catch-all forms. `syn::Pat` is `#[non_exhaustive]`; all
/// known variants are enumerated so the trailing `_` covers only future
/// variants (never a wildcard arm).
fn is_wildcard_arm(pat: &syn::Pat) -> bool {
    match pat {
        syn::Pat::Wild(_) => true,
        syn::Pat::Ident(pi) => {
            pi.subpat.is_none()
                && pi
                    .ident
                    .to_string()
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_lowercase())
        }
        syn::Pat::Const(_)
        | syn::Pat::Lit(_)
        | syn::Pat::Macro(_)
        | syn::Pat::Or(_)
        | syn::Pat::Paren(_)
        | syn::Pat::Path(_)
        | syn::Pat::Range(_)
        | syn::Pat::Reference(_)
        | syn::Pat::Rest(_)
        | syn::Pat::Slice(_)
        | syn::Pat::Struct(_)
        | syn::Pat::Tuple(_)
        | syn::Pat::TupleStruct(_)
        | syn::Pat::Type(_)
        | syn::Pat::Verbatim(_) => false,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    //! RFC-053 §3.1 fixture pattern set — prefix extraction, the three
    //! named recall limits, arm counting, and the wildcard heuristic
    //! (including the lowercase-ident ambiguity).

    use super::*;

    /// Parse `match x { <arms> }` and return its arms.
    fn arms(src: &str) -> Vec<syn::Arm> {
        match syn::parse_str::<syn::Expr>(src).expect("valid match expression") {
            syn::Expr::Match(m) => m.arms,
            other => panic!("expected a match expression, got {other:?}"),
        }
    }

    fn facts(src: &str) -> MatchFacts {
        match_facts(&arms(src))
    }

    /// Parse a single-arm match and return that arm's top-level pattern.
    fn first_pat(src: &str) -> syn::Pat {
        arms(src).into_iter().next().expect("one arm").pat
    }

    #[test]
    fn multi_segment_path_prefix_is_all_but_last_segment() {
        let f = facts("match v { Foo::Bar => () }");
        assert_eq!(f.prefixes, vec!["Foo".to_string()]);
        assert_eq!(f.arm_count, 1);
        assert!(!f.wildcard);
    }

    #[test]
    fn deeply_qualified_prefix_keeps_all_leading_segments() {
        // `syn::Visibility::Public` → prefix `syn::Visibility` (the exact
        // shape of cfdb's own canonical `parse_syn_visibility` site).
        let f = facts("match vis { syn::Visibility::Public(_) => () }");
        assert_eq!(f.prefixes, vec!["syn::Visibility".to_string()]);
    }

    #[test]
    fn nested_some_x_y_skips_the_single_segment_wrapper() {
        // `Some(Visibility::Pub)` → one prefix `Visibility`; the
        // single-segment `Some` wrapper contributes nothing.
        let f = facts("match v { Some(Visibility::Pub) => () }");
        assert_eq!(f.prefixes, vec!["Visibility".to_string()]);
    }

    #[test]
    fn or_patterns_contribute_every_branch_prefix() {
        let f = facts("match v { A::X | B::Y => () }");
        assert_eq!(f.prefixes, vec!["A".to_string(), "B".to_string()]);
    }

    #[test]
    fn reference_pattern_recurses_into_inner_path() {
        let f = facts("match v { &Foo::A => () }");
        assert_eq!(f.prefixes, vec!["Foo".to_string()]);
    }

    #[test]
    fn ident_at_subpattern_recurses_into_the_bound_pattern() {
        // `x @ Foo::A` → the `@` sub-pattern carries the path prefix; the
        // `x` binding itself contributes nothing and is not a wildcard.
        let f = facts("match v { x @ Foo::A => () }");
        assert_eq!(f.prefixes, vec!["Foo".to_string()]);
        assert!(!f.wildcard);
    }

    #[test]
    fn tuple_and_slice_containers_recurse_into_elements() {
        let tuple = facts("match v { (Foo::A, Bar::B) => () }");
        assert_eq!(tuple.prefixes, vec!["Bar".to_string(), "Foo".to_string()]);
        let slice = facts("match v { [Foo::A, Bar::B] => () }");
        assert_eq!(slice.prefixes, vec!["Bar".to_string(), "Foo".to_string()]);
    }

    #[test]
    fn struct_pattern_contributes_its_path_prefix() {
        let f = facts("match v { Foo::Bar { .. } => () }");
        assert_eq!(f.prefixes, vec!["Foo".to_string()]);
    }

    #[test]
    fn single_segment_pattern_is_skipped_recall_limit_1() {
        // A bare uppercase ident (`Pub` under a glob import) parses as a
        // binding-shaped `Pat::Ident` — indistinguishable from a
        // unit-variant path, so no prefix is emitted (recall limit #1).
        let f = facts("match v { Pub => () }");
        assert!(f.prefixes.is_empty());
    }

    #[test]
    fn literal_scrutinee_arms_emit_no_prefix_recall_limit_matches() {
        // The `&str` match in `Visibility::FromStr` — string-literal arm
        // patterns carry no path, so no `:MatchSite` is warranted.
        let f = facts(r#"match s { "pub" => 1, "private" => 2, _ => 0 }"#);
        assert!(f.prefixes.is_empty());
    }

    #[test]
    fn arm_count_reflects_every_arm() {
        let f = facts("match v { Foo::A => 1, Foo::B => 2, _ => 0 }");
        assert_eq!(f.arm_count, 3);
        // The three arms collapse to the single distinct prefix `Foo`.
        assert_eq!(f.prefixes, vec!["Foo".to_string()]);
    }

    #[test]
    fn wildcard_underscore_arm_sets_the_flag() {
        let f = facts("match v { Foo::A => 1, _ => 0 }");
        assert!(f.wildcard);
    }

    #[test]
    fn wildcard_lowercase_binding_sets_the_flag_recall_limit_2() {
        // A bare lowercase binding is a catch-all under the heuristic.
        let f = facts("match v { Foo::A => 1, other => 0 }");
        assert!(f.wildcard);
    }

    #[test]
    fn uppercase_bare_ident_is_not_a_wildcard_recall_limit_2() {
        // Ambiguity case: an uppercase bare ident reads as a
        // variant/const path, not a fresh binding — not flagged.
        assert!(!is_wildcard_arm(&first_pat("match v { None => () }")));
    }

    #[test]
    fn wildcard_underscore_pattern_is_flagged_directly() {
        assert!(is_wildcard_arm(&first_pat("match v { _ => () }")));
    }
}

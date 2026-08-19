use std::collections::BTreeSet;

pub(crate) struct MatchFacts {
    pub(crate) prefixes: Vec<String>,
    pub(crate) arm_count: u32,
    pub(crate) wildcard: bool,
}

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

fn collect_pattern_prefixes(pat: &syn::Pat, out: &mut BTreeSet<String>) {
    match pat {
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
        syn::Pat::Wild(_)
        | syn::Pat::Rest(_)
        | syn::Pat::Lit(_)
        | syn::Pat::Range(_)
        | syn::Pat::Const(_)
        | syn::Pat::Macro(_)
        | syn::Pat::Verbatim(_)
        | syn::Pat::Type(_) => {}
        _ => {}
    }
}

fn push_path_prefix(path: &syn::Path, out: &mut BTreeSet<String>) {
    let segments: Vec<String> = path.segments.iter().map(|s| s.ident.to_string()).collect();
    if segments.len() >= 2 {
        out.insert(segments[..segments.len() - 1].join("::"));
    }
}

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

    use super::*;

    fn arms(src: &str) -> Vec<syn::Arm> {
        match syn::parse_str::<syn::Expr>(src).expect("valid match expression") {
            syn::Expr::Match(m) => m.arms,
            other => panic!("expected a match expression, got {other:?}"),
        }
    }

    fn facts(src: &str) -> MatchFacts {
        match_facts(&arms(src))
    }

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
        let f = facts("match vis { syn::Visibility::Public(_) => () }");
        assert_eq!(f.prefixes, vec!["syn::Visibility".to_string()]);
    }

    #[test]
    fn nested_some_x_y_skips_the_single_segment_wrapper() {
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
        let f = facts("match v { Pub => () }");
        assert!(f.prefixes.is_empty());
    }

    #[test]
    fn literal_scrutinee_arms_emit_no_prefix_recall_limit_matches() {
        let f = facts(r#"match s { "pub" => 1, "private" => 2, _ => 0 }"#);
        assert!(f.prefixes.is_empty());
    }

    #[test]
    fn arm_count_reflects_every_arm() {
        let f = facts("match v { Foo::A => 1, Foo::B => 2, _ => 0 }");
        assert_eq!(f.arm_count, 3);
        assert_eq!(f.prefixes, vec!["Foo".to_string()]);
    }

    #[test]
    fn wildcard_underscore_arm_sets_the_flag() {
        let f = facts("match v { Foo::A => 1, _ => 0 }");
        assert!(f.wildcard);
    }

    #[test]
    fn wildcard_lowercase_binding_sets_the_flag_recall_limit_2() {
        let f = facts("match v { Foo::A => 1, other => 0 }");
        assert!(f.wildcard);
    }

    #[test]
    fn uppercase_bare_ident_is_not_a_wildcard_recall_limit_2() {
        assert!(!is_wildcard_arm(&first_pat("match v { None => () }")));
    }

    #[test]
    fn wildcard_underscore_pattern_is_flagged_directly() {
        assert!(is_wildcard_arm(&first_pat("match v { _ => () }")));
    }
}

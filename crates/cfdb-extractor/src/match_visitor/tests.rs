use std::collections::BTreeSet;

use cfdb_core::fact::{Edge, Node, PropValue};
use cfdb_core::schema::{EdgeLabel, Label};

use super::walk_match_sites_with_test_flag;
use crate::Emitter;

fn walk(block_src: &str) -> (Vec<Node>, Vec<Edge>) {
    walk_with_test_flag(block_src, false)
}

fn walk_with_test_flag(block_src: &str, is_test: bool) -> (Vec<Node>, Vec<Edge>) {
    let block: syn::Block = syn::parse_str(block_src).expect("valid block source");
    let mut emitter = Emitter::new();
    walk_match_sites_with_test_flag(
        &mut emitter,
        "m::f",
        &cfdb_core::qname::TargetDiscriminator::Lib,
        "crates/x/src/lib.rs",
        "x",
        &block,
        is_test,
    );
    emitter.finish()
}

fn match_sites(nodes: &[Node]) -> Vec<&Node> {
    nodes
        .iter()
        .filter(|n| n.label.as_str() == Label::MATCH_SITE)
        .collect()
}

fn prop_str<'a>(n: &'a Node, key: &str) -> &'a str {
    n.props
        .get(key)
        .and_then(PropValue::as_str)
        .unwrap_or_else(|| panic!("string prop {key} present"))
}

#[test]
fn multi_arm_single_prefix_emits_exactly_one_site() {
    let (nodes, _edges) = walk(
        r#"{
            match vis {
                syn::Visibility::Public(_) => 1,
                syn::Visibility::Inherited => 2,
                syn::Visibility::Restricted(r) => 3,
            };
        }"#,
    );
    let sites = match_sites(&nodes);
    assert_eq!(sites.len(), 1, "3 arms, one prefix → one :MatchSite");
    assert_eq!(prop_str(sites[0], "matched_path"), "syn::Visibility");
    assert_eq!(sites[0].id, "matchsite:m::f:syn::Visibility:0");
    assert_eq!(
        sites[0].props.get("arm_count").and_then(PropValue::as_i64),
        Some(3)
    );
}

#[test]
fn multi_prefix_same_expression_emits_distinct_sites() {
    let (nodes, _edges) = walk(
        r#"{
            match pair {
                (Foo::A, _) => 1,
                (_, Bar::B) => 2,
                _ => 3,
            };
        }"#,
    );
    let sites = match_sites(&nodes);
    assert_eq!(sites.len(), 2);

    let mut paths: Vec<&str> = sites.iter().map(|n| prop_str(n, "matched_path")).collect();
    paths.sort();
    assert_eq!(paths, vec!["Bar", "Foo"]);

    let ids: BTreeSet<&str> = sites.iter().map(|n| n.id.as_str()).collect();
    assert!(ids.contains("matchsite:m::f:Foo:0"));
    assert!(ids.contains("matchsite:m::f:Bar:0"));
    assert_eq!(ids.len(), 2, "distinct ids, no collision");

    for s in &sites {
        assert_eq!(
            s.props.get("wildcard").and_then(PropValue::as_bool),
            Some(true),
            "the `_` arm makes the enclosing match a wildcard match"
        );
        assert_eq!(
            s.props.get("arm_count").and_then(PropValue::as_i64),
            Some(3)
        );
    }
}

#[test]
fn same_prefix_in_two_match_expressions_increments_the_counter() {
    let (nodes, _edges) = walk(
        r#"{
            match a { Foo::A => 1, _ => 0 };
            match b { Foo::B => 2, _ => 0 };
        }"#,
    );
    let sites = match_sites(&nodes);
    assert_eq!(sites.len(), 2);
    let ids: BTreeSet<&str> = sites.iter().map(|n| n.id.as_str()).collect();
    assert!(ids.contains("matchsite:m::f:Foo:0"));
    assert!(ids.contains("matchsite:m::f:Foo:1"));
}

#[test]
fn every_match_site_has_a_matches_at_parent() {
    let (nodes, edges) = walk(r#"{ match v { Foo::A => 1, _ => 0 }; }"#);
    let sites = match_sites(&nodes);
    assert!(!sites.is_empty());
    for s in &sites {
        assert!(
            edges
                .iter()
                .any(|e| e.label.as_str() == EdgeLabel::MATCHES_AT
                    && e.dst == s.id
                    && e.src == "item:m::f"),
            "each :MatchSite has a MATCHES_AT edge from item:m::f, missing for {}",
            s.id
        );
    }
}

#[test]
fn match_inside_macro_invocation_is_extracted() {
    let (nodes, _edges) = walk(r#"{ some_macro!(match v { Foo::A => 1, Foo::B => 2 }); }"#);
    let sites = match_sites(&nodes);
    assert_eq!(sites.len(), 1);
    assert_eq!(prop_str(sites[0], "matched_path"), "Foo");
}

#[test]
fn matches_macro_emits_no_site_recall_limit_3() {
    let (nodes, _edges) = walk(r#"{ matches!(v, Foo::A); }"#);
    assert!(match_sites(&nodes).is_empty());
}

#[test]
fn literal_scrutinee_match_emits_nothing() {
    let (nodes, _edges) = walk(r#"{ match s { "a" => 1, "b" => 2, _ => 0 }; }"#);
    assert!(match_sites(&nodes).is_empty());
}

#[test]
fn is_test_flag_propagates_to_every_site() {
    let (nodes, _edges) = walk_with_test_flag(r#"{ match v { Foo::A => 1, _ => 0 }; }"#, true);
    let sites = match_sites(&nodes);
    assert!(!sites.is_empty());
    assert!(sites
        .iter()
        .all(|n| n.props.get("is_test").and_then(PropValue::as_bool) == Some(true)));
}

#[test]
fn nested_match_in_arm_body_is_extracted() {
    let (nodes, _edges) = walk(
        r#"{
            match outer {
                Foo::A => match inner { Bar::B => 1, _ => 0 },
                _ => 0,
            };
        }"#,
    );
    let mut paths: Vec<&str> = match_sites(&nodes)
        .iter()
        .map(|n| prop_str(n, "matched_path"))
        .collect();
    paths.sort();
    assert_eq!(paths, vec!["Bar", "Foo"]);
}

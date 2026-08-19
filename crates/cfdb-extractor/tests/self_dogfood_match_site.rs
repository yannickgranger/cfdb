use std::path::Path;

use cfdb_core::fact::PropValue;
use cfdb_core::schema::{EdgeLabel, Label};
use cfdb_extractor::extract_workspace;

fn cfdb_workspace_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("cfdb-extractor crate dir has parent (crates/)")
        .parent()
        .expect("crates/ has parent (cfdb sub-workspace root)")
}

fn prop_str<'a>(node: &'a cfdb_core::fact::Node, key: &str) -> Option<&'a str> {
    node.props.get(key).and_then(PropValue::as_str)
}

fn is_production(node: &cfdb_core::fact::Node) -> bool {
    node.props.get("is_test").and_then(PropValue::as_bool) == Some(false)
}

#[test]
fn exactly_one_syn_visibility_match_site_in_item_visitor() {
    let (nodes, _edges) = extract_workspace(cfdb_workspace_root()).expect("extract cfdb");

    let hits: Vec<&cfdb_core::fact::Node> = nodes
        .iter()
        .filter(|n| n.label.as_str() == Label::MATCH_SITE && is_production(n))
        .filter(|n| prop_str(n, "matched_path") == Some("syn::Visibility"))
        .filter(|n| {
            prop_str(n, "file").is_some_and(|f| f.ends_with("cfdb-extractor/src/item_visitor.rs"))
        })
        .collect();

    assert_eq!(
        hits.len(),
        1,
        "expected exactly ONE production :MatchSite with matched_path = \
         'syn::Visibility' in item_visitor.rs (parse_syn_visibility, the \
         self-documented canonical site); got {hits:?}"
    );
}

#[test]
fn visibility_enum_match_site_present_and_from_str_str_match_absent() {
    let (nodes, _edges) = extract_workspace(cfdb_workspace_root()).expect("extract cfdb");

    let in_visibility_rs: Vec<&cfdb_core::fact::Node> = nodes
        .iter()
        .filter(|n| n.label.as_str() == Label::MATCH_SITE && is_production(n))
        .filter(|n| prop_str(n, "file").is_some_and(|f| f.ends_with("cfdb-core/src/visibility.rs")))
        .collect();

    let visibility_hits = in_visibility_rs
        .iter()
        .filter(|n| prop_str(n, "matched_path") == Some("Visibility"))
        .count();
    assert!(
        visibility_hits >= 1,
        "expected >=1 production :MatchSite with matched_path = 'Visibility' \
         in visibility.rs (Visibility::as_wire_str); got {visibility_hits}"
    );

    for n in &in_visibility_rs {
        assert_eq!(
            prop_str(n, "matched_path"),
            Some("Visibility"),
            "the only production :MatchSite in visibility.rs must come from \
             as_wire_str (matched_path 'Visibility'); a different value means \
             FromStr's &str match wrongly emitted a site: {n:?}"
        );
    }
}

#[test]
fn every_match_site_has_a_matches_at_parent() {
    let (nodes, edges) = extract_workspace(cfdb_workspace_root()).expect("extract cfdb");

    let match_dst: std::collections::BTreeSet<&str> = edges
        .iter()
        .filter(|e| e.label.as_str() == EdgeLabel::MATCHES_AT)
        .map(|e| e.dst.as_str())
        .collect();

    let sites: Vec<&cfdb_core::fact::Node> = nodes
        .iter()
        .filter(|n| n.label.as_str() == Label::MATCH_SITE)
        .collect();
    assert!(
        !sites.is_empty(),
        "cfdb's own tree must contain at least one :MatchSite"
    );
    for s in &sites {
        assert!(
            match_dst.contains(s.id.as_str()),
            ":MatchSite {} has no incoming MATCHES_AT edge",
            s.id
        );
    }
}

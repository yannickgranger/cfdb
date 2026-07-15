//! RFC-053 slice 53-A self-dogfood — extract cfdb's own sub-workspace via
//! `extract_workspace` and assert `:MatchSite` / `MATCHES_AT` against the
//! two source-verified canonical sites in cfdb's own tree (RFC-053 §4:
//! rustdoc-json carries no match-expression facts, so `cfdb-recall`'s
//! oracle cannot cover this fact kind — self-dogfood is the substitute).
//!
//! Canonical sites at slice-ship time:
//! - `crates/cfdb-extractor/src/item_visitor.rs` `parse_syn_visibility`
//!   matches `syn::Visibility` (an EXTERNAL type — the exact "per-site
//!   fact whose target may not resolve" instance the RFC exists for) →
//!   exactly ONE `:MatchSite` with `matched_path = "syn::Visibility"`.
//! - `crates/cfdb-core/src/visibility.rs` `Visibility::as_wire_str`
//!   matches `self` (the WORKSPACE enum) → `matched_path = "Visibility"`.
//! - `Visibility::FromStr::from_str` matches a `&str` — literal-scrutinee,
//!   no path prefix → emits NO `:MatchSite` (so the only production sites
//!   in visibility.rs carry `matched_path = "Visibility"`).

use std::path::Path;

use cfdb_core::fact::PropValue;
use cfdb_core::schema::{EdgeLabel, Label};
use cfdb_extractor::extract_workspace;

/// Resolve the cfdb sub-workspace root — this crate's grandparent directory.
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

    // `as_wire_str`'s match on `self` yields the workspace-enum prefix.
    let visibility_hits = in_visibility_rs
        .iter()
        .filter(|n| prop_str(n, "matched_path") == Some("Visibility"))
        .count();
    assert!(
        visibility_hits >= 1,
        "expected >=1 production :MatchSite with matched_path = 'Visibility' \
         in visibility.rs (Visibility::as_wire_str); got {visibility_hits}"
    );

    // `FromStr::from_str` matches a `&str` — a literal scrutinee with no
    // path prefix. It must emit NO site, so EVERY production site in
    // visibility.rs carries matched_path = 'Visibility' (the as_wire_str
    // one). Any other value would mean the &str match leaked a site.
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

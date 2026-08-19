use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use cfdb_core::fact::{Node, PropValue};
use cfdb_core::schema::{EdgeLabel, Label};
use cfdb_extractor::extract_workspace;

fn cfdb_workspace_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("cfdb-extractor crate dir has parent (crates/)")
        .parent()
        .expect("crates/ has parent (cfdb sub-workspace root)")
}

fn prop_str<'a>(node: &'a Node, key: &str) -> Option<&'a str> {
    node.props.get(key).and_then(PropValue::as_str)
}

fn is_production(node: &Node) -> bool {
    node.props.get("is_test").and_then(PropValue::as_bool) == Some(false)
}

#[test]
fn visibility_enum_has_incoming_matches_on_from_visibility_rs() {
    let (nodes, edges) = extract_workspace(cfdb_workspace_root()).expect("extract cfdb");
    let by_id: BTreeMap<&str, &Node> = nodes.iter().map(|n| (n.id.as_str(), n)).collect();

    let hits = edges
        .iter()
        .filter(|e| e.label.as_str() == EdgeLabel::MATCHES_ON)
        .filter(|e| {
            by_id
                .get(e.src.as_str())
                .and_then(|n| prop_str(n, "file"))
                .is_some_and(|f| f.ends_with("cfdb-core/src/visibility.rs"))
        })
        .filter(|e| {
            by_id.get(e.dst.as_str()).is_some_and(|n| {
                n.label.as_str() == Label::ITEM
                    && prop_str(n, "kind") == Some("enum")
                    && prop_str(n, "name") == Some("Visibility")
            })
        })
        .count();

    assert!(
        hits >= 1,
        "cfdb_core's Visibility enum must have >=1 incoming MATCHES_ON from a \
         :MatchSite in cfdb-core/src/visibility.rs (Visibility::as_wire_str's \
         unqualified `Visibility::…` match resolves to the workspace enum); \
         got {hits}"
    );
}

#[test]
fn syn_visibility_site_has_no_matches_on_edge() {
    let (nodes, edges) = extract_workspace(cfdb_workspace_root()).expect("extract cfdb");

    let syn_vis_site_ids: BTreeSet<&str> = nodes
        .iter()
        .filter(|n| n.label.as_str() == Label::MATCH_SITE && is_production(n))
        .filter(|n| prop_str(n, "matched_path") == Some("syn::Visibility"))
        .filter(|n| {
            prop_str(n, "file").is_some_and(|f| f.ends_with("cfdb-extractor/src/item_visitor.rs"))
        })
        .map(|n| n.id.as_str())
        .collect();

    assert!(
        !syn_vis_site_ids.is_empty(),
        "precondition: the 53-A syn::Visibility :MatchSite must exist in \
         item_visitor.rs (parse_syn_visibility)"
    );

    for e in edges
        .iter()
        .filter(|e| e.label.as_str() == EdgeLabel::MATCHES_ON)
    {
        assert!(
            !syn_vis_site_ids.contains(e.src.as_str()),
            "the external syn::Visibility site {} must have NO MATCHES_ON edge \
             — a qualified external prefix must not resolve onto the same-named \
             workspace Visibility enum (RFC-053 §3.5 homonym guard)",
            e.src
        );
    }
}

#[test]
fn every_matches_on_edge_targets_an_enum() {
    let (nodes, edges) = extract_workspace(cfdb_workspace_root()).expect("extract cfdb");
    let by_id: BTreeMap<&str, &Node> = nodes.iter().map(|n| (n.id.as_str(), n)).collect();

    let matches_on: Vec<_> = edges
        .iter()
        .filter(|e| e.label.as_str() == EdgeLabel::MATCHES_ON)
        .collect();
    assert!(
        !matches_on.is_empty(),
        "cfdb's own tree must contain at least one resolved MATCHES_ON edge \
         (Visibility::as_wire_str), else this invariant is vacuous"
    );

    for e in &matches_on {
        let target: &Node = by_id
            .get(e.dst.as_str())
            .copied()
            .unwrap_or_else(|| panic!("MATCHES_ON dst {} must be an emitted node", e.dst));
        assert_eq!(
            prop_str(target, "kind"),
            Some("enum"),
            "MATCHES_ON dst {} must be an :Item{{kind:\"enum\"}} (RFC-053 §3.2 \
             kind filter); got kind {:?}",
            e.dst,
            prop_str(target, "kind")
        );
    }
}

use super::*;

fn qnames(
    items: &[&str],
) -> std::collections::BTreeMap<String, Vec<cfdb_core::qname::TargetDiscriminator>> {
    items
        .iter()
        .map(|s| {
            (
                s.to_string(),
                vec![cfdb_core::qname::TargetDiscriminator::Lib],
            )
        })
        .collect()
}

#[test]
fn resolve_type_string_exact_qname_hit() {
    let items = qnames(&["mycrate::Color"]);
    let index = build_last_segment_index(&items);
    assert_eq!(
        resolve_type_string(&items, &index, "mycrate::Color"),
        Some("mycrate::Color".to_string())
    );
}

#[test]
fn resolve_type_string_unique_last_segment_hit() {
    let items = qnames(&["mycrate::Color"]);
    let index = build_last_segment_index(&items);
    assert_eq!(
        resolve_type_string(&items, &index, "Color"),
        Some("mycrate::Color".to_string())
    );
}

#[test]
fn resolve_type_string_ambiguous_last_segment_drops() {
    let items = qnames(&["a::Color", "b::Color"]);
    let index = build_last_segment_index(&items);
    assert_eq!(resolve_type_string(&items, &index, "Color"), None);
}

#[test]
fn build_last_segment_index_marks_unique_and_ambiguous() {
    let items = qnames(&["a::Foo", "b::Bar", "a::Dup", "c::Dup"]);
    let index = build_last_segment_index(&items);
    assert_eq!(
        index.get("Foo").copied().flatten(),
        Some(&"a::Foo".to_string())
    );
    assert_eq!(
        index.get("Bar").copied().flatten(),
        Some(&"b::Bar".to_string())
    );
    assert_eq!(index.get("Dup").copied(), Some(None));
}

fn resolve_one(
    item_qnames: &[&str],
    enum_qnames: &[&str],
    site_id: &str,
    prefix: &str,
) -> Vec<Edge> {
    let mut emitter = Emitter::new();
    emitter.emitted_item_qnames = qnames(item_qnames);
    emitter.emitted_enum_qnames = qnames(enum_qnames);
    emitter.deferred_match_targets.push((
        site_id.to_string(),
        prefix.to_string(),
        cfdb_core::qname::TargetDiscriminator::Lib,
    ));

    resolve_deferred_match_targets(&mut emitter);

    let (_nodes, edges) = emitter.finish();
    edges
        .into_iter()
        .filter(|e| e.label.as_str() == EdgeLabel::MATCHES_ON)
        .collect()
}

#[test]
fn matches_on_exact_qname_enum_hit_emits_edge() {
    let edges = resolve_one(
        &["mycrate::Color"],
        &["mycrate::Color"],
        "matchsite:mycrate::f:mycrate::Color:0",
        "mycrate::Color",
    );
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].src, "matchsite:mycrate::f:mycrate::Color:0");
    assert_eq!(
        edges[0].dst,
        cfdb_core::qname::item_node_id("mycrate::Color")
    );
}

#[test]
fn matches_on_unique_last_segment_enum_hit_emits_edge() {
    let edges = resolve_one(
        &["mycrate::Color"],
        &["mycrate::Color"],
        "matchsite:mycrate::f:Color:0",
        "Color",
    );
    assert_eq!(edges.len(), 1);
    assert_eq!(
        edges[0].dst,
        cfdb_core::qname::item_node_id("mycrate::Color")
    );
}

#[test]
fn matches_on_ambiguous_prefix_drops() {
    let edges = resolve_one(
        &["a::Color", "b::Color"],
        &["a::Color", "b::Color"],
        "matchsite:a::f:Color:0",
        "Color",
    );
    assert!(edges.is_empty());
}

#[test]
fn matches_on_struct_prefix_drops() {
    let edges = resolve_one(
        &["mycrate::Shape"],
        &[],
        "matchsite:mycrate::f:mycrate::Shape:0",
        "mycrate::Shape",
    );
    assert!(edges.is_empty());
}

#[test]
fn matches_on_external_prefix_yields_node_without_edge() {
    let edges = resolve_one(
        &["mycrate::Visibility"],
        &["mycrate::Visibility"],
        "matchsite:cfdb_extractor::parse_syn_visibility:syn::Visibility:0",
        "syn::Visibility",
    );
    assert!(
        edges.is_empty(),
        "qualified external prefix syn::Visibility must not collapse onto \
         the same-named workspace enum via last-segment (§3.5 homonym-proof)"
    );
}

#[test]
fn matches_on_unqualified_homonym_still_resolves() {
    let edges = resolve_one(
        &["mycrate::Visibility"],
        &["mycrate::Visibility"],
        "matchsite:mycrate::as_wire_str:Visibility:0",
        "Visibility",
    );
    assert_eq!(edges.len(), 1);
    assert_eq!(
        edges[0].dst,
        cfdb_core::qname::item_node_id("mycrate::Visibility")
    );
}

#[test]
fn matches_on_partially_qualified_in_crate_prefix_resolves() {
    let edges = resolve_one(
        &["mycrate::mymod::MyEnum"],
        &["mycrate::mymod::MyEnum"],
        "matchsite:mycrate::f:mymod::MyEnum:0",
        "mymod::MyEnum",
    );
    assert_eq!(edges.len(), 1);
    assert_eq!(
        edges[0].dst,
        cfdb_core::qname::item_node_id("mycrate::mymod::MyEnum")
    );
}

#[test]
fn matches_on_qualified_non_suffix_prefix_rejected_despite_unique_last_segment() {
    let edges = resolve_one(
        &["mycrate::deep::Config"],
        &["mycrate::deep::Config"],
        "matchsite:mycrate::f:other::Config:0",
        "other::Config",
    );
    assert!(
        edges.is_empty(),
        "a qualified prefix whose segments are not a suffix of the \
         resolved qname must be rejected even when the last segment is \
         unique (§3.5 homonym guard)"
    );
}

#[test]
fn is_segment_suffix_matches_trailing_segments_only() {
    assert!(is_segment_suffix("mycrate::Color", "mycrate::Color"));
    assert!(is_segment_suffix("Color", "mycrate::Color"));
    assert!(is_segment_suffix(
        "visibility::Visibility",
        "cfdb_core::visibility::Visibility"
    ));
    assert!(!is_segment_suffix(
        "syn::Visibility",
        "cfdb_core::visibility::Visibility"
    ));
    assert!(!is_segment_suffix("a::b::Color", "mycrate::Color"));
    assert!(!is_segment_suffix(
        "bility",
        "cfdb_core::visibility::Visibility"
    ));
}

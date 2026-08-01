//! Unit tests for the post-walk resolvers — the MATCHES_ON resolution
//! pass (RFC-053 §3.2, slice 53-B) plus the first direct coverage of the
//! shared `resolve_type_string` / `build_last_segment_index` primitives
//! (prescribed 53-B byproduct). Extracted to a sibling file to keep
//! `resolver.rs` under the architecture god-file ceiling.
//!
//! The parent `resolver.rs` declares this module via
//! `#[cfg(test)] mod tests;`, so this file does NOT carry its own
//! `#![cfg(test)]` — that would be a duplicate-attribute clippy
//! violation (rust-1.93 `clippy::duplicated_attributes`). `use super::*`
//! sees the private `resolve_type_string` / `build_last_segment_index` /
//! `is_segment_suffix` siblings, so the primitive tests need no
//! visibility widening (RFC-053 §3.2).

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

// ---- resolve_type_string / build_last_segment_index primitives ----
// First direct coverage of the shared resolver primitives (prescribed
// 53-B byproduct — previously exercised only end-to-end via RETURNS /
// TYPE_OF).

#[test]
fn resolve_type_string_exact_qname_hit() {
    let items = qnames(&["mycrate::Color"]);
    let index = build_last_segment_index(&items);
    // An already-qualified prefix hits tier 1 (exact) directly.
    assert_eq!(
        resolve_type_string(&items, &index, "mycrate::Color"),
        Some("mycrate::Color".to_string())
    );
}

#[test]
fn resolve_type_string_unique_last_segment_hit() {
    let items = qnames(&["mycrate::Color"]);
    let index = build_last_segment_index(&items);
    // A bare `Color` prefix misses tier 1 but hits tier 2 via the
    // unique last-segment index.
    assert_eq!(
        resolve_type_string(&items, &index, "Color"),
        Some("mycrate::Color".to_string())
    );
}

#[test]
fn resolve_type_string_ambiguous_last_segment_drops() {
    let items = qnames(&["a::Color", "b::Color"]);
    let index = build_last_segment_index(&items);
    // Two workspace `Color`s make the last segment ambiguous — tier 2
    // drops silently rather than mis-attributing.
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
    // `Dup` declared in two crates → ambiguous → None.
    assert_eq!(index.get("Dup").copied(), Some(None));
}

// ---- resolve_deferred_match_targets resolution fixtures ----
// A `:MatchSite` is modelled by its id + queued prefix; the resolver
// emits MATCHES_ON only when the prefix resolves to a workspace enum.

/// Drive the resolver over a single `(site_id, prefix)` deferred entry
/// against the given item / enum sets, returning the MATCHES_ON edges
/// it emitted (0 or 1).
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
    // Arm written `Color::Red` → prefix `Color`; resolves via the
    // unique last-segment tier to the workspace enum.
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
    // Both `Color`s are enums, but the ambiguous last segment drops in
    // the shared primitive before the kind filter is even consulted.
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
    // Prefix resolves (exact) to a workspace `:Item`, but that item is
    // a struct — absent from the enum set — so the kind filter drops
    // it (RFC-053 §3.2: struct destructuring is not dispatch).
    let edges = resolve_one(
        &["mycrate::Shape"],
        &[], // Shape is a struct, not an enum
        "matchsite:mycrate::f:mycrate::Shape:0",
        "mycrate::Shape",
    );
    assert!(edges.is_empty());
}

#[test]
fn matches_on_external_prefix_yields_node_without_edge() {
    // The homonym-proofness case (RFC-053 §3.5), and the exact cfdb-self
    // shape: a workspace enum `mycrate::Visibility` exists AND a
    // `:MatchSite` matches the EXTERNAL `syn::Visibility` (like
    // `parse_syn_visibility`). Both share the last segment `Visibility`,
    // so an unguarded last-segment fallback would WRONGLY resolve the
    // external site onto the workspace enum. `["syn","Visibility"]` is
    // not a segment-suffix of `["mycrate","Visibility"]`, so the homonym
    // guard rejects it and emits no MATCHES_ON edge. The `:MatchSite`
    // node itself is emitted walk-time by `match_visitor` (out of this
    // resolver's scope): the honest name-level-only representation is a
    // site node with no edge.
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
    // Complement to the guard: the SAME workspace enum, matched by its
    // UNQUALIFIED name (`Visibility::Public`, like `as_wire_str` on the
    // real Visibility enum) → prefix `Visibility` → last-segment hit →
    // MATCHES_ON edge. The guard drops qualified externals WITHOUT
    // suppressing the legitimate unqualified workspace-enum dispatch.
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
    // A module-qualified but not crate-qualified arm (`mymod::MyEnum::A`)
    // → prefix `mymod::MyEnum`. resolve_type_string finds the enum via
    // its unique last segment; `["mymod","MyEnum"]` IS a segment-suffix
    // of `["mycrate","mymod","MyEnum"]`, so the guard KEEPS it. This is
    // the in-crate recall the earlier "reject all qualified" guard
    // dropped (RFC-053 §4 "never silently absorbed").
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
    // The workspace enum `mycrate::deep::Config` has a UNIQUE last
    // segment `Config`, so resolve_type_string's last-segment tier
    // returns it for the external `other::Config` prefix. But
    // `["other","Config"]` is not a segment-suffix of
    // `["mycrate","deep","Config"]` (the qualifier `other` != `deep`),
    // so the homonym guard rejects it — no edge, even though the last
    // segment resolved uniquely.
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
    // Exact, unqualified, and partial-in-crate all pass.
    assert!(is_segment_suffix("mycrate::Color", "mycrate::Color"));
    assert!(is_segment_suffix("Color", "mycrate::Color"));
    assert!(is_segment_suffix(
        "visibility::Visibility",
        "cfdb_core::visibility::Visibility"
    ));
    // A different qualifier on the same last segment does NOT.
    assert!(!is_segment_suffix(
        "syn::Visibility",
        "cfdb_core::visibility::Visibility"
    ));
    // A prefix longer than the qname cannot be a suffix.
    assert!(!is_segment_suffix("a::b::Color", "mycrate::Color"));
    // Raw-string suffix that is not a SEGMENT boundary must NOT count.
    assert!(!is_segment_suffix(
        "bility",
        "cfdb_core::visibility::Visibility"
    ));
}

//! Tests for the post-walk synthesis pass.
//!
//! The parent `synthesize.rs` declares this module via
//! `#[cfg(test)] mod tests;`, so this file does NOT carry its own
//! `#![cfg(test)]` — that would be a duplicate-attribute clippy
//! violation.

use super::*;
use cfdb_core::fact::Edge;

/// Empty overrides — for foreign crates (`std`, `serde`,...) the
/// `compute_bounded_context` heuristic returns the bare crate name,
/// which is what the unit tests assert on. Real callers pass the
/// workspace's `.cfdb/concepts/*.toml`-derived overrides.
fn empty_overrides() -> ConceptOverrides {
    ConceptOverrides::default()
}

fn edge(src: &str, dst_qname: &str, label: &'static str) -> Edge {
    Edge {
        src: src.to_string(),
        dst: item_node_id(dst_qname),
        label: EdgeLabel::new(label),
        props: Props::new(),
    }
}

#[test]
fn kind_for_evidence_implements_is_trait() {
    assert_eq!(kind_for_evidence(EdgeLabel::IMPLEMENTS), "trait");
}

#[test]
fn kind_for_evidence_implements_for_is_struct() {
    assert_eq!(kind_for_evidence(EdgeLabel::IMPLEMENTS_FOR), "struct");
}

#[test]
fn kind_for_evidence_returns_is_struct() {
    assert_eq!(kind_for_evidence(EdgeLabel::RETURNS), "struct");
}

#[test]
fn kind_for_evidence_type_of_is_struct() {
    assert_eq!(kind_for_evidence(EdgeLabel::TYPE_OF), "struct");
}

#[test]
fn crate_from_qname_multi_segment() {
    assert_eq!(crate_from_qname("std::fmt::Display"), "std");
    assert_eq!(crate_from_qname("serde::ser::Serialize"), "serde");
}

#[test]
fn crate_from_qname_degenerate_single_segment() {
    // A bare type name (single-segment qname) — degenerate but valid.
    assert_eq!(crate_from_qname("Foo"), "Foo");
}

#[test]
fn promotion_implements_for_then_implements() {
    let mut emitter = Emitter::new();
    emitter.claim_item_qname(
        "crate_a::Source",
        &cfdb_core::qname::TargetDiscriminator::Lib,
    );
    emitter.emit_edge(edge(
        &item_node_id("crate_a::Source"),
        "std::fmt::Display",
        EdgeLabel::IMPLEMENTS_FOR,
    ));
    emitter.emit_edge(edge(
        &item_node_id("crate_a::Source"),
        "std::fmt::Display",
        EdgeLabel::IMPLEMENTS,
    ));

    synthesize_referenced_items(&mut emitter, &empty_overrides());

    let (nodes, _edges) = emitter.finish();
    let display = nodes
        .iter()
        .find(|n| n.id == item_node_id("std::fmt::Display"))
        .expect("synthesised :Item for Display present");
    assert_eq!(
        display.props.get("kind").and_then(PropValue::as_str),
        Some("trait"),
        "IMPLEMENTS evidence promotes over IMPLEMENTS_FOR fallback"
    );
}

#[test]
fn promotion_implements_then_implements_for() {
    // Insertion order reversed — IMPLEMENTS first, IMPLEMENTS_FOR
    // after. Trait evidence MUST be sticky.
    let mut emitter = Emitter::new();
    emitter.claim_item_qname(
        "crate_a::Source",
        &cfdb_core::qname::TargetDiscriminator::Lib,
    );
    emitter.emit_edge(edge(
        &item_node_id("crate_a::Source"),
        "std::fmt::Display",
        EdgeLabel::IMPLEMENTS,
    ));
    emitter.emit_edge(edge(
        &item_node_id("crate_a::Source"),
        "std::fmt::Display",
        EdgeLabel::IMPLEMENTS_FOR,
    ));

    synthesize_referenced_items(&mut emitter, &empty_overrides());

    let (nodes, _edges) = emitter.finish();
    let display = nodes
        .iter()
        .find(|n| n.id == item_node_id("std::fmt::Display"))
        .expect("synthesised :Item for Display present");
    assert_eq!(
        display.props.get("kind").and_then(PropValue::as_str),
        Some("trait"),
        "IMPLEMENTS evidence is sticky regardless of insertion order"
    );
}

#[test]
fn dedup_two_implements_for_yields_single_node() {
    // Same qname referenced twice as IMPLEMENTS_FOR — must produce
    // exactly one synthesised :Item node, not two.
    let mut emitter = Emitter::new();
    emitter.claim_item_qname("crate_a::A", &cfdb_core::qname::TargetDiscriminator::Lib);
    emitter.claim_item_qname("crate_a::B", &cfdb_core::qname::TargetDiscriminator::Lib);
    emitter.emit_edge(edge(
        &item_node_id("crate_a::A"),
        "ext::Foo",
        EdgeLabel::IMPLEMENTS_FOR,
    ));
    emitter.emit_edge(edge(
        &item_node_id("crate_a::B"),
        "ext::Foo",
        EdgeLabel::IMPLEMENTS_FOR,
    ));

    synthesize_referenced_items(&mut emitter, &empty_overrides());

    let (nodes, _edges) = emitter.finish();
    let foo_count = nodes
        .iter()
        .filter(|n| n.id == item_node_id("ext::Foo"))
        .count();
    assert_eq!(foo_count, 1, "qname dedup");
}

#[test]
fn synthesises_one_node_with_minimal_props() {
    let mut emitter = Emitter::new();
    emitter.claim_item_qname(
        "crate_a::MyType",
        &cfdb_core::qname::TargetDiscriminator::Lib,
    );
    emitter.emit_edge(edge(
        &item_node_id("crate_a::MyType"),
        "std::fmt::Display",
        EdgeLabel::IMPLEMENTS,
    ));

    synthesize_referenced_items(&mut emitter, &empty_overrides());

    let (nodes, _edges) = emitter.finish();
    let display = nodes
        .iter()
        .find(|n| n.id == item_node_id("std::fmt::Display"))
        .expect("Display synthesised");
    assert_eq!(
        display.props.get("qname").and_then(PropValue::as_str),
        Some("std::fmt::Display")
    );
    assert_eq!(
        display.props.get("name").and_then(PropValue::as_str),
        Some("Display")
    );
    assert_eq!(
        display.props.get("kind").and_then(PropValue::as_str),
        Some("trait")
    );
    assert_eq!(
        display.props.get("crate").and_then(PropValue::as_str),
        Some("std")
    );
    // `compute_bounded_context("std", empty_overrides)` falls through
    // to the heuristic which returns the crate name unchanged for
    // crates with no recognised prefix.
    assert_eq!(
        display
            .props
            .get("bounded_context")
            .and_then(PropValue::as_str),
        Some("std")
    );
    // Body-shaped props are deliberately ABSENT — that is the
    // discriminator between walked and synthesised items.
    for absent in [
        "file",
        "visibility",
        "module_qpath",
        "line",
        "signature",
        "signature_hash",
        "is_test",
        "is_deprecated",
        "doc_text",
    ] {
        assert!(
            !display.props.contains_key(absent),
            "synthesised :Item must NOT carry `{}` prop (absence = discriminator)",
            absent
        );
    }
}

#[test]
fn idempotent_on_re_run() {
    let mut emitter = Emitter::new();
    emitter.claim_item_qname("crate_a::A", &cfdb_core::qname::TargetDiscriminator::Lib);
    emitter.emit_edge(edge(
        &item_node_id("crate_a::A"),
        "ext::Foo",
        EdgeLabel::IMPLEMENTS_FOR,
    ));

    synthesize_referenced_items(&mut emitter, &empty_overrides());
    synthesize_referenced_items(&mut emitter, &empty_overrides());

    let (nodes, _edges) = emitter.finish();
    let foo_count = nodes
        .iter()
        .filter(|n| n.id == item_node_id("ext::Foo"))
        .count();
    assert_eq!(foo_count, 1, "second pass is a no-op");
}

#[test]
fn skips_walked_qnames() {
    // A workspace-internal qname is in emitted_item_qnames; the
    // synthesis pass must NOT add a second :Item for it.
    let mut emitter = Emitter::new();
    emitter.claim_item_qname("crate_a::A", &cfdb_core::qname::TargetDiscriminator::Lib);
    emitter.claim_item_qname(
        "cfdb_extractor::Foo",
        &cfdb_core::qname::TargetDiscriminator::Lib,
    );
    emitter.emit_edge(edge(
        &item_node_id("crate_a::A"),
        "cfdb_extractor::Foo",
        EdgeLabel::IMPLEMENTS_FOR,
    ));

    synthesize_referenced_items(&mut emitter, &empty_overrides());

    let (nodes, _edges) = emitter.finish();
    let foo_synth = nodes
        .iter()
        .filter(|n| n.id == item_node_id("cfdb_extractor::Foo"))
        .count();
    assert_eq!(foo_synth, 0, "walked qname must not be re-emitted");
}

#[test]
fn covers_all_four_edge_labels() {
    // Same-shape edges with each of the four labels target four
    // distinct foreign qnames. Each must produce a synthesised
    // :Item with the correct kind.
    let mut emitter = Emitter::new();
    emitter.claim_item_qname(
        "crate_a::Source",
        &cfdb_core::qname::TargetDiscriminator::Lib,
    );
    for (dst_qname, label) in [
        ("ext::TraitImpl", EdgeLabel::IMPLEMENTS),
        ("ext::ImplFor", EdgeLabel::IMPLEMENTS_FOR),
        ("ext::RetType", EdgeLabel::RETURNS),
        ("ext::FieldType", EdgeLabel::TYPE_OF),
    ] {
        emitter.emit_edge(edge(&item_node_id("crate_a::Source"), dst_qname, label));
    }

    synthesize_referenced_items(&mut emitter, &empty_overrides());

    let (nodes, _edges) = emitter.finish();
    let by_id: BTreeMap<&str, &Node> = nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    assert_eq!(
        by_id
            .get(item_node_id("ext::TraitImpl").as_str())
            .and_then(|n| n.props.get("kind"))
            .and_then(PropValue::as_str),
        Some("trait")
    );
    for (dst, want_kind) in [
        ("ext::ImplFor", "struct"),
        ("ext::RetType", "struct"),
        ("ext::FieldType", "struct"),
    ] {
        let got = by_id
            .get(item_node_id(dst).as_str())
            .and_then(|n| n.props.get("kind"))
            .and_then(PropValue::as_str);
        assert_eq!(got, Some(want_kind), "label-to-kind for dst={}", dst);
    }
}

#[test]
fn ignores_unrelated_edge_labels() {
    // Edges with labels NOT in {IMPLEMENTS, IMPLEMENTS_FOR, RETURNS,
    // TYPE_OF} must not trigger synthesis even if dst qname is
    // unwalked.
    let mut emitter = Emitter::new();
    emitter.claim_item_qname(
        "crate_a::Source",
        &cfdb_core::qname::TargetDiscriminator::Lib,
    );
    emitter.emit_edge(edge(
        &item_node_id("crate_a::Source"),
        "ext::Foo",
        EdgeLabel::HAS_FIELD,
    ));
    emitter.emit_edge(edge(
        &item_node_id("crate_a::Source"),
        "ext::Bar",
        EdgeLabel::CALLS,
    ));

    synthesize_referenced_items(&mut emitter, &empty_overrides());

    let (nodes, _edges) = emitter.finish();
    let synth_count = nodes
        .iter()
        .filter(|n| n.label == Label::new(Label::ITEM))
        .count();
    assert_eq!(synth_count, 0, "non-scope labels do not synthesise");
}

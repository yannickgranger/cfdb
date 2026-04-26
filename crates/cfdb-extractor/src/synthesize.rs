//! Post-walk synthesis pass — emit minimal `:Item` nodes for every edge
//! dst qname that no walk path emitted (issue #317).
//!
//! ## Why
//!
//! The walk emits IMPLEMENTS / IMPLEMENTS_FOR / RETURNS / TYPE_OF edges
//! whose dst qnames may live outside the walked workspace —
//! `std::fmt::Display`, `serde::Serialize`, `thiserror::Error`. Without a
//! corresponding `:Item` node, [`cfdb_petgraph::graph::ingest_one_edge`]
//! treats the edge's dst id as unknown and silently drops the edge
//! (cfdb-petgraph/src/graph.rs::ingest_one_edge → "unknown dst id, edge
//! skipped"). Result on cfdb-self pre-fix: 0 IMPLEMENTS edges survive.
//!
//! ## What
//!
//! After both [`crate::resolver::resolve_deferred_returns`] and
//! [`crate::resolver::resolve_deferred_type_of`] have run — and therefore
//! every edge that will ever land in the workspace is in
//! [`Emitter::edges`] — synthesise one minimal `:Item` per distinct dst
//! qname that is NOT already in [`Emitter::emitted_item_qnames`]. The
//! synthesised nodes carry only the always-required identity props
//! (`qname`, `name`, `kind`, `crate`, `bounded_context`); body-shaped
//! props (`file`, `visibility`, `module_qpath`, `line`, `signature`,
//! `is_test`, `is_deprecated`, ...) are deliberately omitted — their
//! absence is the discriminator between "walked from source" and
//! "referenced only" (commit `fdaf333` rationale, RFC-039 withdrawal).
//!
//! ## Kind inference (AC-4 + AC-5)
//!
//! Edge label pins the dst's kind by the Rust grammar:
//!
//! - `IMPLEMENTS` dst → `kind = "trait"` (grammar requires)
//! - `IMPLEMENTS_FOR` / `RETURNS` / `TYPE_OF` dst → `kind = "struct"`
//!   (fallback for type-position references — could also be enum or
//!   union, but `struct` is the most general type-shaped value the
//!   `ItemKind` vocabulary supports)
//!
//! When the same qname appears under multiple labels, IMPLEMENTS wins —
//! it is the only label that proves trait-ness. Other labels only prove
//! "some type." Monotone aggregation: order-independent.
//!
//! ## Idempotency
//!
//! Skips qnames already in `emitter.emitted_item_qnames` so a
//! workspace-internal item is never duplicated by a second synthesised
//! node. Updates `emitted_item_qnames` for each synthesised qname so
//! re-running the pass on the same Emitter is a no-op.

use std::collections::BTreeMap;

use cfdb_concepts::{compute_bounded_context, ConceptOverrides};
use cfdb_core::fact::{Node, PropValue, Props};
use cfdb_core::qname::{item_node_id, last_segment, qname_from_node_id};
use cfdb_core::query::item_kind::ItemKind;
use cfdb_core::schema::{EdgeLabel, Label};

use crate::emitter::Emitter;

/// Emit a minimal `:Item` node for every edge dst qname that no walk
/// path emitted. Runs once, after both deferred-edge resolvers, before
/// [`Emitter::finish`]. See module docs for design rationale.
///
/// `overrides` are passed through to [`compute_bounded_context`] so the
/// synthesised `:Item.bounded_context` value matches what the walk-time
/// path would have written if the qname's crate had been walked. Same
/// resolution point as the walked items — the
/// [`cfdb_petgraph::enrich::bounded_context`] re-enrichment pass therefore
/// stays a no-op on synthesised nodes (otherwise it would patch every
/// synthesised entry, breaking AC-2 of the cfdb-scoped self-dogfood).
pub(crate) fn synthesize_referenced_items(emitter: &mut Emitter, overrides: &ConceptOverrides) {
    // Step 1 — collect dst qnames keyed by strongest evidence so far.
    // IMPLEMENTS sticky-overwrites; the others only insert if absent.
    let mut synth: BTreeMap<String, &'static str> = BTreeMap::new();
    for edge in emitter.edges() {
        let label = edge.label.as_str();
        let evidence = match label {
            EdgeLabel::IMPLEMENTS => EdgeLabel::IMPLEMENTS,
            EdgeLabel::IMPLEMENTS_FOR => EdgeLabel::IMPLEMENTS_FOR,
            EdgeLabel::RETURNS => EdgeLabel::RETURNS,
            EdgeLabel::TYPE_OF => EdgeLabel::TYPE_OF,
            _ => continue,
        };
        let dst_qname = qname_from_node_id(&edge.dst);
        if emitter.emitted_item_qnames.contains(dst_qname) {
            continue;
        }
        match synth.get(dst_qname) {
            Some(&existing) if existing == EdgeLabel::IMPLEMENTS => {
                // Already promoted to trait — nothing stronger to apply.
            }
            _ if evidence == EdgeLabel::IMPLEMENTS => {
                // Promote (or insert) trait-evidence.
                synth.insert(dst_qname.to_string(), evidence);
            }
            None => {
                // Insert fallback evidence; future IMPLEMENTS may promote.
                synth.insert(dst_qname.to_string(), evidence);
            }
            Some(_) => {
                // Existing fallback evidence stays — no demotion.
            }
        }
    }

    // Step 2 — emit one :Item per qname. Update emitted_item_qnames so a
    // second pass over the same Emitter is a no-op.
    //
    // bounded_context is computed via the same `compute_bounded_context`
    // helper the walk-time path uses (override-first, heuristic-fallback).
    // For foreign crates ("std", "serde") with no override entry the
    // heuristic returns the bare crate name; for in-workspace crates that
    // happen to be referenced before walking the override produces the
    // correct context. Memoised by crate name to keep complexity at
    // O(distinct crates) rather than O(qnames).
    let mut bc_memo: BTreeMap<String, String> = BTreeMap::new();
    for (qname, evidence) in synth {
        let kind = kind_for_evidence(evidence);
        let name = last_segment(&qname).to_string();
        let crate_name = crate_from_qname(&qname);
        let bounded_context = bc_memo
            .entry(crate_name.clone())
            .or_insert_with(|| compute_bounded_context(&crate_name, overrides).name)
            .clone();

        let mut props: Props = BTreeMap::new();
        props.insert("qname".to_string(), PropValue::Str(qname.clone()));
        props.insert("name".to_string(), PropValue::Str(name));
        props.insert("kind".to_string(), PropValue::Str(kind.to_string()));
        props.insert("crate".to_string(), PropValue::Str(crate_name));
        props.insert(
            "bounded_context".to_string(),
            PropValue::Str(bounded_context),
        );

        emitter.emit_node(Node {
            id: item_node_id(&qname),
            label: Label::new(Label::ITEM),
            props,
        });
        emitter.emitted_item_qnames.insert(qname);
    }
}

/// Map edge-label evidence to the synthesised `:Item.kind` value.
///
/// AC-4: kind is derived from the edge label that brought the qname
/// into scope. IMPLEMENTS proves the dst is a trait; the others
/// (IMPLEMENTS_FOR, RETURNS, TYPE_OF) only prove the dst is some
/// type, so we fall back to `struct` (the most general type-shaped
/// value the `ItemKind` vocabulary supports — RFC-cfdb §7).
///
/// Routes through [`ItemKind::to_extractor_str`] (RATIFIED.md §A.14)
/// so any future rename of the wire-form vocabulary stays single-sourced.
fn kind_for_evidence(evidence: &'static str) -> &'static str {
    match evidence {
        EdgeLabel::IMPLEMENTS => ItemKind::Trait.to_extractor_str(),
        _ => ItemKind::Struct.to_extractor_str(),
    }
}

/// First `::`-delimited segment of a qname — the bare crate name a
/// minimal synthesised `:Item.crate` carries. Single-segment qnames
/// (degenerate but valid for foreign single-name types) map crate to
/// the qname itself.
fn crate_from_qname(qname: &str) -> String {
    qname
        .split_once("::")
        .map(|(c, _)| c.to_string())
        .unwrap_or_else(|| qname.to_string())
}

#[cfg(test)]
mod tests {
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
        emitter.emitted_item_qnames.insert("crate_a::Source".into());
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
        emitter.emitted_item_qnames.insert("crate_a::Source".into());
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
        emitter.emitted_item_qnames.insert("crate_a::A".into());
        emitter.emitted_item_qnames.insert("crate_a::B".into());
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
        emitter.emitted_item_qnames.insert("crate_a::MyType".into());
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
        emitter.emitted_item_qnames.insert("crate_a::A".into());
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
        emitter.emitted_item_qnames.insert("crate_a::A".to_string());
        emitter
            .emitted_item_qnames
            .insert("cfdb_extractor::Foo".to_string());
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
        emitter
            .emitted_item_qnames
            .insert("crate_a::Source".to_string());
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
        emitter
            .emitted_item_qnames
            .insert("crate_a::Source".to_string());
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
}

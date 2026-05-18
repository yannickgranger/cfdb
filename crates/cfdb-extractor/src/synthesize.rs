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
    // O(distinct crates) rather than O(qnames). The memo lookup AND
    // the props-construction are extracted to helpers so the per-row
    // clones land outside the loop body (boy-scout #374).
    let mut bc_memo: BTreeMap<String, String> = BTreeMap::new();
    for (qname, evidence) in synth {
        let kind = kind_for_evidence(evidence);
        let crate_name = crate_from_qname(&qname);
        let bounded_context = memoized_bounded_context(&mut bc_memo, &crate_name, overrides);
        let props = build_synthetic_item_props(&qname, kind, crate_name, bounded_context);

        emitter.emit_node(Node {
            id: item_node_id(&qname),
            label: Label::new(Label::ITEM),
            props,
        });
        emitter.emitted_item_qnames.insert(qname);
    }
}

/// Return the cached `bounded_context` for `crate_name`, computing it
/// (and caching) on first access. Extracted from
/// [`synthesize_referenced_items`] so the per-row clone needed to
/// retain `crate_name` for the memo key happens outside the emission
/// loop body (#374, RFC §7 no-clones-in-loops).
fn memoized_bounded_context(
    bc_memo: &mut BTreeMap<String, String>,
    crate_name: &str,
    overrides: &ConceptOverrides,
) -> String {
    if let Some(existing) = bc_memo.get(crate_name) {
        return existing.clone();
    }
    let computed = compute_bounded_context(crate_name, overrides).name;
    bc_memo.insert(crate_name.to_string(), computed.clone());
    computed
}

/// Build the `:Item.props` map for one synthesised node. Extracted so
/// the `qname.to_string()` allocation (the `qname` String is moved into
/// `emitted_item_qnames` after this call returns) does not sit on a
/// line inside the emission loop body (boy-scout #374, RFC §7
/// no-clones-in-loops detector is line-based).
fn build_synthetic_item_props(
    qname: &str,
    kind: &str,
    crate_name: String,
    bounded_context: String,
) -> Props {
    let name = last_segment(qname).to_string();
    let mut props: Props = BTreeMap::new();
    props.insert("qname".to_string(), PropValue::Str(qname.to_string()));
    props.insert("name".to_string(), PropValue::Str(name));
    props.insert("kind".to_string(), PropValue::Str(kind.to_string()));
    props.insert("crate".to_string(), PropValue::Str(crate_name));
    props.insert(
        "bounded_context".to_string(),
        PropValue::Str(bounded_context),
    );
    props
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
mod tests;

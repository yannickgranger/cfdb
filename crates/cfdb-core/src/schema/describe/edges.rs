//! Edge-label descriptors for `schema_describe()`.

use super::super::descriptors::{attr, EdgeLabelDescriptor, Provenance};
use super::super::labels::{EdgeLabel, Label};

pub(super) fn edge_descriptors() -> Vec<EdgeLabelDescriptor> {
    use Provenance::Extractor;
    vec![
        // ---- Structural ------------------------------------------------------
        EdgeLabelDescriptor {
            label: EdgeLabel::new(EdgeLabel::IN_CRATE),
            description: "Any node with a crate belongs to that Crate.".into(),
            attributes: vec![],
            from: vec![],
            to: vec![Label::new(Label::CRATE)],
            provenance: Provenance::Extractor,
        },
        EdgeLabelDescriptor {
            label: EdgeLabel::new(EdgeLabel::IN_MODULE),
            description: "An Item or File is contained in a Module.".into(),
            attributes: vec![],
            from: vec![Label::new(Label::ITEM), Label::new(Label::FILE)],
            to: vec![Label::new(Label::MODULE)],
            provenance: Provenance::Extractor,
        },
        EdgeLabelDescriptor {
            label: EdgeLabel::new(EdgeLabel::HAS_FIELD),
            description: "A struct Item or enum Variant owns a Field.".into(),
            attributes: vec![],
            from: vec![Label::new(Label::ITEM), Label::new(Label::VARIANT)],
            to: vec![Label::new(Label::FIELD)],
            provenance: Provenance::Extractor,
        },
        EdgeLabelDescriptor {
            label: EdgeLabel::new(EdgeLabel::HAS_VARIANT),
            description: "An enum Item owns a Variant.".into(),
            attributes: vec![],
            from: vec![Label::new(Label::ITEM)],
            to: vec![Label::new(Label::VARIANT)],
            provenance: Provenance::Extractor,
        },
        EdgeLabelDescriptor {
            label: EdgeLabel::new(EdgeLabel::HAS_PARAM),
            description: "An fn Item owns a Param.".into(),
            attributes: vec![],
            from: vec![Label::new(Label::ITEM)],
            to: vec![Label::new(Label::PARAM)],
            provenance: Provenance::Extractor,
        },
        EdgeLabelDescriptor {
            label: EdgeLabel::new(EdgeLabel::TYPE_OF),
            description: "A Field, Param, or Variant payload references an Item used as its type."
                .into(),
            attributes: vec![],
            from: vec![
                Label::new(Label::FIELD),
                Label::new(Label::PARAM),
                Label::new(Label::VARIANT),
            ],
            to: vec![Label::new(Label::ITEM)],
            provenance: Provenance::Extractor,
        },
        EdgeLabelDescriptor {
            label: EdgeLabel::new(EdgeLabel::IMPLEMENTS),
            description: "An impl Item implements a trait Item.".into(),
            attributes: vec![],
            from: vec![Label::new(Label::ITEM)],
            to: vec![Label::new(Label::ITEM)],
            provenance: Provenance::Extractor,
        },
        EdgeLabelDescriptor {
            label: EdgeLabel::new(EdgeLabel::IMPLEMENTS_FOR),
            description: "An impl Item targets a type Item (the receiver of the impl).".into(),
            attributes: vec![],
            from: vec![Label::new(Label::ITEM)],
            to: vec![Label::new(Label::ITEM)],
            provenance: Provenance::Extractor,
        },
        EdgeLabelDescriptor {
            label: EdgeLabel::new(EdgeLabel::RETURNS),
            description: "An fn Item returns a type Item.".into(),
            attributes: vec![],
            from: vec![Label::new(Label::ITEM)],
            to: vec![Label::new(Label::ITEM)],
            provenance: Provenance::Extractor,
        },
        EdgeLabelDescriptor {
            label: EdgeLabel::new(EdgeLabel::BELONGS_TO),
            description: "A Crate belongs to its bounded Context (council-cfdb-wiring §B.1.3)."
                .into(),
            attributes: vec![],
            from: vec![Label::new(Label::CRATE)],
            to: vec![Label::new(Label::CONTEXT)],
            provenance: Provenance::Extractor,
        },
        // ---- Call graph ------------------------------------------------------
        EdgeLabelDescriptor {
            label: EdgeLabel::new(EdgeLabel::CALLS),
            description: "Static call edge between two fn Items (best-effort cross-crate).".into(),
            attributes: vec![attr(
                "resolved",
                "bool",
                "`true` when the dispatch was resolved via HIR type inference (`cfdb-hir-extractor`, v0.2+); `false` for textual / unresolved baseline. SchemaVersion v0.1.4+ only. The HIR-based extractor is the first producer of :CALLS edges — v0.1.3 and earlier graphs have no CALLS edges at all.",
                Extractor,
            )],
            from: vec![Label::new(Label::ITEM)],
            to: vec![Label::new(Label::ITEM)],
            provenance: Provenance::Extractor,
        },
        EdgeLabelDescriptor {
            label: EdgeLabel::new(EdgeLabel::INVOKES_AT),
            description: "A CallSite invokes a concrete fn Item.".into(),
            attributes: vec![],
            from: vec![Label::new(Label::CALL_SITE)],
            to: vec![Label::new(Label::ITEM)],
            provenance: Provenance::Extractor,
        },
        // ---- Entry points ----------------------------------------------------
        EdgeLabelDescriptor {
            label: EdgeLabel::new(EdgeLabel::EXPOSES),
            description: "An EntryPoint dispatches to a handler fn Item.".into(),
            attributes: vec![],
            from: vec![Label::new(Label::ENTRY_POINT)],
            to: vec![Label::new(Label::ITEM)],
            provenance: Provenance::Extractor,
        },
        EdgeLabelDescriptor {
            label: EdgeLabel::new(EdgeLabel::REGISTERS_PARAM),
            description: "An EntryPoint declares an entry-point-exposed input — \
                          an MCP tool fn param (:Param), a clap `#[arg]` struct \
                          field (:Field), or a clap `Subcommand` variant (:Variant). \
                          Nodes on the target side keep their structural labels; \
                          this edge carries the semantic that the target is \
                          externally-facing."
                .into(),
            attributes: vec![],
            from: vec![Label::new(Label::ENTRY_POINT)],
            to: vec![
                Label::new(Label::PARAM),
                Label::new(Label::FIELD),
                Label::new(Label::VARIANT),
            ],
            provenance: Provenance::Extractor,
        },
        // ---- Concept overlay -------------------------------------------------
        EdgeLabelDescriptor {
            label: EdgeLabel::new(EdgeLabel::LABELED_AS),
            description: "An Item carries a semantic Concept label.".into(),
            attributes: vec![],
            from: vec![Label::new(Label::ITEM)],
            to: vec![Label::new(Label::CONCEPT)],
            provenance: Provenance::Extractor,
        },
        EdgeLabelDescriptor {
            label: EdgeLabel::new(EdgeLabel::CANONICAL_FOR),
            description: "An Item is the designated authoritative implementation of a Concept."
                .into(),
            attributes: vec![],
            from: vec![Label::new(Label::ITEM)],
            to: vec![Label::new(Label::CONCEPT)],
            provenance: Provenance::Extractor,
        },
        EdgeLabelDescriptor {
            label: EdgeLabel::new(EdgeLabel::EQUIVALENT_TO),
            description: "Reserved — two Concepts are synonyms (e.g. \
                          `TradeSide ≡ Direction`); no producer in v0.x, \
                          planned for Phase B (see issue #307)."
                .into(),
            attributes: vec![],
            from: vec![Label::new(Label::CONCEPT)],
            to: vec![Label::new(Label::CONCEPT)],
            provenance: Provenance::Reserved,
        },
        // ---- Enrichment overlay (RFC addendum §A2.2 — #43-A reservations) ---
        EdgeLabelDescriptor {
            label: EdgeLabel::new(EdgeLabel::REFERENCED_BY),
            description: "An Item is mentioned (by `name` or `qname`) in an RFC document. Emitted by `enrich_rfc_docs()` — slice 43-D (issue #107) ships the first emissions with a SchemaVersion patch bump.".into(),
            attributes: vec![],
            from: vec![Label::new(Label::ITEM)],
            to: vec![Label::new(Label::RFC_DOC)],
            provenance: Provenance::Extractor,
        },
    ]
}

//! Self-documenting schema descriptor types (SOLID-5).
//!
//! These types form the shape of the document returned by
//! [`super::schema_describe`]: a runtime-readable contract covering every
//! node label, edge label, attribute, and provenance in the cfdb graph.

use serde::{Deserialize, Serialize};

use super::labels::{EdgeLabel, Label};
use super::version::SchemaVersion;

/// Where an attribute's value originates. Each value in the cfdb graph has
/// exactly one source — either the structural extract (Layer 1, syn AST +
/// cargo_metadata) or one of the enrichment passes (Layer 2). The provenance
/// is recorded per attribute so consumers can reason about which parts of the
/// graph are machine-derived vs human-curated, and which enrichment passes
/// must have run before a given query is answerable.
///
/// SOLID-5: consumers depend on this abstract provenance vocabulary, not on a
/// specific extractor version — a new extractor implementation can replace
/// the old one as long as it honors the contract advertised by
/// [`super::schema_describe`].
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Provenance {
    /// Structural fact walked directly from the `syn` AST or `cargo_metadata`
    /// during `extract()`. Available immediately after extract — no enrichment
    /// pass required. `is_deprecated` + `deprecation_since` are extractor-time
    /// facts — the `#[deprecated]` attribute is syntactic and the AST walker
    /// already visits attributes.
    Extractor,
    /// Pulled by `enrich_rfc_docs()` — scans `docs/rfc/*.md` and
    /// `.concept-graph/*.md` for concept-name matches and emits `:RfcDoc`
    /// nodes + `(:Item)-[:REFERENCED_BY]->(:RfcDoc)` edges.
    EnrichRfcDocs,
    /// Computed by quality tools during `enrich_metrics()` —
    /// `unwrap_count`, `test_coverage`, `cyclomatic`, `dup_cluster_id`.
    /// Deferred — retained so a future implementation can resuscitate the
    /// pass without a breaking provenance rename.
    EnrichMetrics,
    /// Pulled from `git log` by `enrich_git_history()` —
    /// `git_last_commit_unix_ts`, `git_last_author`, `git_commit_count`.
    EnrichGitHistory,
    /// Assigned by concept rules during `enrich_concepts()` — `:Concept`
    /// node materialization from `.cfdb/concepts/*.toml` declarations, plus
    /// `LABELED_AS` and `CANONICAL_FOR` edges.
    EnrichConcepts,
    /// Written by `enrich_reachability()` — `:Item.reachable_from_entry`,
    /// `:Item.reachable_entry_count` from BFS over `CALLS*` starting at
    /// `:EntryPoint` nodes.
    EnrichReachability,
    /// Reserved for future implementation. Emitted by the schema describer to
    /// mark labels (and, in future, attributes) that are declared in the
    /// vocabulary but not yet wired to any extractor or enrich pass. Distinct
    /// from the producer variants — it represents a fundamentally different
    /// relationship (no producer at all, by design).
    Reserved,
}

/// Description of one attribute on a node or edge label: name, type hint,
/// one-line meaning, and provenance.
///
/// `type_hint` is a short string drawn from a small vocabulary — `"string"`,
/// `"int"`, `"bool"`, `"string?"` (nullable), `"json"` (structured), `"enum"`
/// (documented as a closed set in `description`). It is intentionally not a
/// strict type language; cfdb's on-wire values are the 5-variant `PropValue`
/// and the hint is documentation for consumers, not a parse schema.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AttributeDescriptor {
    pub name: String,
    pub type_hint: String,
    pub description: String,
    pub provenance: Provenance,
}

/// Description of one node label — its canonical label, one-line meaning, and
/// the full attribute list in canonical (sorted-by-name) order.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NodeLabelDescriptor {
    pub label: Label,
    pub description: String,
    pub attributes: Vec<AttributeDescriptor>,
}

/// Description of one edge label — its canonical label, one-line meaning,
/// attribute list, allowed source/target node labels, and the producer
/// provenance. `from` and `to` are empty when the edge is polymorphic (e.g.
/// `IN_CRATE` accepts any node that has a crate). `provenance` names the
/// extractor or enrich pass that emits the edge — or [`Provenance::Reserved`]
/// when the label is declared in the vocabulary without a current producer
/// (issue #307); RFC-037 §3.7 edge-liveness check skips reserved labels.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EdgeLabelDescriptor {
    pub label: EdgeLabel,
    pub description: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attributes: Vec<AttributeDescriptor>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub from: Vec<Label>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub to: Vec<Label>,
    /// Producer of this edge label. Required field (always serializes), but
    /// `#[serde(default)]` defaults to [`Provenance::Extractor`] for
    /// keyspaces serialized before this field landed (issue #307). New
    /// descriptor instances MUST set the field explicitly.
    #[serde(default = "default_edge_provenance")]
    pub provenance: Provenance,
}

/// Backward-compat default for [`EdgeLabelDescriptor::provenance`] — every
/// pre-#307 edge descriptor was extractor-emitted, so `Extractor` is the safe
/// default for serialized data missing the field. Newly written descriptors
/// must set `provenance:` explicitly so `Provenance::Reserved` is never
/// silently selected.
fn default_edge_provenance() -> Provenance {
    Provenance::Extractor
}

/// The self-describing schema document returned by [`super::schema_describe`].
/// RFC §6A.1 exposes this as the `schema_describe()` verb in the SCHEMA verb
/// group. Consumers (LLMs, skill adapters, query writers) read this instead
/// of hardcoding the vocabulary against a specific extractor version.
///
/// The document is deterministic and byte-stable for a given cfdb-core build:
/// calling [`super::schema_describe`] twice in the same process produces
/// identical output, supporting G1 (canonical dump stability, RFC §6A.3).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SchemaDescribe {
    pub schema_version: SchemaVersion,
    pub nodes: Vec<NodeLabelDescriptor>,
    pub edges: Vec<EdgeLabelDescriptor>,
}

/// Internal helper: build an [`AttributeDescriptor`] from string slices. Used
/// by the node/edge descriptor builders in [`super::describe`].
pub(super) fn attr(
    name: &str,
    type_hint: &str,
    description: &str,
    provenance: Provenance,
) -> AttributeDescriptor {
    AttributeDescriptor {
        name: name.to_string(),
        type_hint: type_hint.to_string(),
        description: description.to_string(),
        provenance,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provenance_round_trips_as_snake_case() {
        for p in [
            Provenance::Extractor,
            Provenance::EnrichRfcDocs,
            Provenance::EnrichMetrics,
            Provenance::EnrichGitHistory,
            Provenance::EnrichConcepts,
            Provenance::EnrichReachability,
            Provenance::Reserved,
        ] {
            let json = serde_json::to_string(&p).expect("Provenance is a plain derived enum");
            let back: Provenance =
                serde_json::from_str(&json).expect("round-trip of just-serialized Provenance");
            assert_eq!(p, back);
        }
        // Spot-check snake_case renames land on the pass vocabulary.
        assert_eq!(
            serde_json::to_string(&Provenance::EnrichRfcDocs)
                .expect("Provenance is a plain derived enum"),
            "\"enrich_rfc_docs\""
        );
        assert_eq!(
            serde_json::to_string(&Provenance::EnrichGitHistory)
                .expect("Provenance is a plain derived enum"),
            "\"enrich_git_history\""
        );
        assert_eq!(
            serde_json::to_string(&Provenance::EnrichReachability)
                .expect("Provenance is a plain derived enum"),
            "\"enrich_reachability\""
        );
        assert_eq!(
            serde_json::to_string(&Provenance::Reserved)
                .expect("Provenance is a plain derived enum"),
            "\"reserved\""
        );
    }
}

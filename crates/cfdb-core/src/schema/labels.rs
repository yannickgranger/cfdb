//! Label newtypes — node/edge labels, keyspace, schema version.
//!
//! RFC §7 defines the ten node labels and ~20 edge labels. This module encodes
//! them as plain strings wrapped in newtypes so the extractor, parser, and
//! evaluator can share a single vocabulary without stringly-typing it.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Canonical node label (RFC §7). Free-form string so v0.2+ extensions do not
/// require a cfdb-core release; well-known labels are provided as constants.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Label(pub String);

impl Label {
    pub const CRATE: &'static str = "Crate";
    pub const MODULE: &'static str = "Module";
    pub const FILE: &'static str = "File";
    pub const ITEM: &'static str = "Item";
    pub const FIELD: &'static str = "Field";
    pub const VARIANT: &'static str = "Variant";
    pub const PARAM: &'static str = "Param";
    /// A single call expression in the source.
    ///
    /// **Published discriminator contract (SchemaVersion v0.1.3+).** Every
    /// `:CallSite` node MUST carry two discriminator properties:
    ///
    /// - `resolver: string` — the extractor that produced this node.
    ///   Valid values: `"syn"` (unresolved, name-based — `cfdb-extractor`)
    ///   or `"hir"` (resolved via HIR type inference — `cfdb-hir-extractor`,
    ///   RFC-029 §A1.2 Phase B, v0.2+).
    /// - `callee_resolved: bool` — `false` when the callee path is textual
    ///   only; `true` when method dispatch / re-export / trait impl was
    ///   resolved via HIR.
    ///
    /// These discriminate the homonym that arises once both extractors emit
    /// `:CallSite` into the same graph. Queries filter on these properties
    /// to select the appropriate population. See RFC-029 §A1.2 (homonym
    /// mitigation) and issue #83.
    pub const CALL_SITE: &'static str = "CallSite";
    pub const ENTRY_POINT: &'static str = "EntryPoint";
    pub const CONCEPT: &'static str = "Concept";
    pub const CONTEXT: &'static str = "Context";
    /// An RFC document file (`docs/rfc/*.md`, `.concept-graph/*.md`, etc.)
    /// referenced by concept-name matching during `enrich_rfc_docs()`.
    /// Reserved in #43-A; first emissions land in slice 43-D (issue #107)
    /// alongside the `REFERENCED_BY` edge and a SchemaVersion patch bump.
    /// `:RfcDoc` nodes carry `path` (string, workspace-relative) and
    /// optional `title` (string, from the first `# ` heading).
    pub const RFC_DOC: &'static str = "RfcDoc";

    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Label {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for Label {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// Canonical edge label (RFC §7).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EdgeLabel(pub String);

impl EdgeLabel {
    // Structural
    pub const IN_CRATE: &'static str = "IN_CRATE";
    pub const IN_MODULE: &'static str = "IN_MODULE";
    pub const HAS_FIELD: &'static str = "HAS_FIELD";
    pub const HAS_VARIANT: &'static str = "HAS_VARIANT";
    pub const HAS_PARAM: &'static str = "HAS_PARAM";
    pub const TYPE_OF: &'static str = "TYPE_OF";
    pub const IMPLEMENTS: &'static str = "IMPLEMENTS";
    pub const IMPLEMENTS_FOR: &'static str = "IMPLEMENTS_FOR";
    pub const RETURNS: &'static str = "RETURNS";
    pub const BELONGS_TO: &'static str = "BELONGS_TO";

    // Call graph
    pub const CALLS: &'static str = "CALLS";
    pub const INVOKES_AT: &'static str = "INVOKES_AT";

    // Entry points
    pub const EXPOSES: &'static str = "EXPOSES";
    pub const REGISTERS_PARAM: &'static str = "REGISTERS_PARAM";

    // Concept overlay
    pub const LABELED_AS: &'static str = "LABELED_AS";
    pub const CANONICAL_FOR: &'static str = "CANONICAL_FOR";
    pub const EQUIVALENT_TO: &'static str = "EQUIVALENT_TO";

    // Enrichment-time overlay (RFC addendum §A2.2 — #43-A reservations)
    /// `(:Item)-[:REFERENCED_BY]->(:RfcDoc)` — set when an item's `name`
    /// or `qname` is matched in an RFC document during `enrich_rfc_docs()`.
    /// Reserved in #43-A; first emissions land in slice 43-D (issue #107).
    pub const REFERENCED_BY: &'static str = "REFERENCED_BY";

    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for EdgeLabel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for EdgeLabel {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// A keyspace identifies one indexed workspace (RFC §9 multi-project support).
/// Typically the workspace name (e.g. `"qbot-core"`).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Keyspace(pub String);

impl Keyspace {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Keyspace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Semantic version of the fact schema in a keyspace. G4 (RFC §6) requires
/// monotonic compatibility within a major — v1.1 graphs are queryable by v1.0
/// consumers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SchemaVersion {
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
}

impl SchemaVersion {
    pub const V0_1_0: Self = Self {
        major: 0,
        minor: 1,
        patch: 0,
    };

    /// v0.1.1 — Issue #35 adds `:Item.visibility` (additive, non-breaking
    /// within the 0.x major; V0_1_0 readers refuse V0_1_1 graphs per G4).
    pub const V0_1_1: Self = Self {
        major: 0,
        minor: 1,
        patch: 1,
    };

    /// v0.1.2 — Issue #36 adds the optional `:Item.cfg_gate` attribute
    /// carrying the `#[cfg(feature = "…")]` expression tree captured on
    /// the item (absent when the item has no feature-cfg). Additive and
    /// non-breaking within 0.x.
    pub const V0_1_2: Self = Self {
        major: 0,
        minor: 1,
        patch: 2,
    };

    /// v0.1.3 — Issue #83 adds the `:CallSite.resolver` and
    /// `:CallSite.callee_resolved` discriminator properties so the syn
    /// (unresolved) and HIR (resolved) extractors can both emit
    /// `:CallSite` into the same graph without homonym ambiguity
    /// (RFC-029 §A1.2). Every `cfdb-extractor`-emitted `:CallSite`
    /// carries `resolver="syn"` + `callee_resolved=false`; every
    /// `cfdb-hir-extractor`-emitted `:CallSite` (v0.2+) will carry
    /// `resolver="hir"` + `callee_resolved=true`. Additive and
    /// non-breaking within 0.x.
    pub const V0_1_3: Self = Self {
        major: 0,
        minor: 1,
        patch: 3,
    };

    /// v0.1.4 — Issue #94 adds the `CALLS.resolved: bool` edge
    /// attribute distinguishing HIR-resolved dispatch (true, emitted
    /// by `cfdb-hir-extractor` v0.2+) from syn-based textual calls
    /// (false, reserved for future unresolved-baseline emission). The
    /// #94 slice also wires the first resolved `CALLS` +
    /// `INVOKES_AT` emissions end-to-end through the
    /// `cfdb-hir-petgraph-adapter`. Additive and non-breaking within
    /// 0.x — V0_1_3 graphs have no `CALLS` edges emitted at all (the
    /// syn extractor doesn't emit them; the HIR extractor is the
    /// first producer).
    pub const V0_1_4: Self = Self {
        major: 0,
        minor: 1,
        patch: 4,
    };

    /// v0.2.0 — Issue #86 completes the v0.2 feature set per RFC-032
    /// / RFC-029 §A1.2. First emissions of `:EntryPoint` nodes and
    /// `EXPOSES` edges (MVP detects clap `#[derive(Parser/Subcommand)]`
    /// on structs/enums and `#[tool]` on fns — MCP + CLI coverage per
    /// v0.2-1 acceptance gate). Also introduces the `cfdb-cli
    /// --features hir` composition seam: default CLI builds remain
    /// ra-ap-* free; the HIR pipeline is opt-in (RFC-032 §3 lines
    /// 221–227). Minor bump (not patch) marks the v0.2 capability
    /// boundary — V0_1_4 readers refuse V0_2_0 graphs per G4, which
    /// is the intended signal since V0_2_0 graphs may contain
    /// `:EntryPoint` / `EXPOSES` facts that V0_1_4 readers don't know
    /// how to handle.
    pub const V0_2_0: Self = Self {
        major: 0,
        minor: 2,
        patch: 0,
    };

    /// v0.2.1 — Issue #106 (Slice 43-C) lands the first extractor-time
    /// emissions of `:Item.is_deprecated` (bool, always emitted) and
    /// `:Item.deprecation_since` (string, emitted only when the
    /// `#[deprecated(since = "X")]` form is used). Both attributes were
    /// reserved in #104 (Slice 43-A) with `Provenance::Extractor`; #106
    /// adds the `extract_deprecated_attr` helper and wires it through
    /// `emit_item_with_flags` + the impl-method visitor path.
    /// Additive and non-breaking within 0.2.x — V0_2_0 readers loading
    /// a V0_2_1 keyspace ignore the extra item properties.
    /// First patch bump under the post-#43-A per-slice bump policy;
    /// ships with a lockstep `graph-specs-rust` cross-fixture PR per
    /// cfdb CLAUDE.md §3.
    pub const V0_2_1: Self = Self {
        major: 0,
        minor: 2,
        patch: 1,
    };

    /// v0.2.2 — Issue #42 lands first emissions of impl-block `:Item`
    /// nodes (`kind = "impl_block"`) plus `IMPLEMENTS` and
    /// `IMPLEMENTS_FOR` edges. Both edge labels were reserved in
    /// `labels.rs` and described in `describe.rs` from v0.1 onwards but
    /// no extractor produced them. `cfdb-extractor::visit_item_impl`
    /// now emits, per `impl ... {}` block: (a) a `:Item { kind:
    /// "impl_block" }` node with a qname of shape
    /// `<module>::<target>::impl[_<trait>]`, (b) an `IMPLEMENTS_FOR`
    /// edge pointing at the target type's `:Item`, and (c) for trait
    /// impls only, an `IMPLEMENTS` edge pointing at the trait's
    /// `:Item`. Additive and non-breaking within 0.2.x. Pre-V0_2_2
    /// keyspaces carry zero `impl_block` items. Paired lockstep
    /// `graph-specs-rust` cross-fixture bump per cfdb CLAUDE.md §3.
    pub const V0_2_2: Self = Self {
        major: 0,
        minor: 2,
        patch: 2,
    };

    /// v0.2.3 — Issue #107 (Slice 43-D) lands first emissions of `:RfcDoc`
    /// nodes and `(:Item)-[:REFERENCED_BY]->(:RfcDoc)` edges. Both were
    /// descriptor-reserved in slice 43-A (issue #104) but no enrichment
    /// pass produced them. `PetgraphStore::enrich_rfc_docs` now scans
    /// workspace `docs/**/*.md` and `.concept-graph/*.md` for whole-word
    /// matches on every `:Item`'s `name` and `qname`, emitting one
    /// `:RfcDoc { path, title }` per referenced file and one
    /// `REFERENCED_BY` edge per (item, file) pair. Additive and
    /// non-breaking within 0.2.x — V0_2_2 readers loading a V0_2_3
    /// keyspace see extra nodes and edges they do not understand and
    /// ignore them. Paired lockstep `graph-specs-rust` cross-fixture
    /// bump per cfdb CLAUDE.md §3.
    pub const V0_2_3: Self = Self {
        major: 0,
        minor: 2,
        patch: 3,
    };

    /// **v0.3.0 — RFC-037 schema-producer alignment epoch.** Captures
    /// all RFC-037 deltas landed in PRs #224 (#215 qname helpers +
    /// #216 RETURNS producer + #217 `:Field` attr alignment), #225
    /// (#218 `:Variant` producer + `HAS_VARIANT` + `emit_field_list` +
    /// widened `HAS_FIELD` descriptor), #226 (#219 REGISTERS_PARAM
    /// 3-paths with widened `to: [:Param, :Field, :Variant]` + #220
    /// TYPE_OF producer), and this slice's vestigial deletions of
    /// `SUPERTRAIT` + `RECEIVES_ARG` (both declared in v0.1 with no
    /// producer or consumer; removed per RFC-037 §3.6 cleanup).
    ///
    /// **Breaking changes carried by this minor bump:**
    /// - `:Field.type_qname` prop REMOVED (replaced by `type_normalized` + `type_path`); V0_2_3 readers loading a V0_3_0 keyspace no longer see `type_qname`.
    /// - `EdgeLabel::SUPERTRAIT` + `EdgeLabel::RECEIVES_ARG` constants REMOVED; no keyspace on disk ever carried these labels but the API surface is reduced.
    ///
    /// **Additive facts carried by this bump:**
    /// - `:Variant` nodes + `HAS_VARIANT` edges (enum variants now walked — previously dormant).
    /// - `:Field` tuple-struct + tuple-variant + struct-variant fields (previously only `Fields::Named` on structs emitted).
    /// - `HAS_FIELD.from` widened to `[:Item, :Variant]`.
    /// - `:Field` attrs gain `index`, `type_normalized`, `type_path`.
    /// - `RETURNS` edges (producer shipped in #216).
    /// - `TYPE_OF` edges (producer shipped in #220).
    /// - `REGISTERS_PARAM.to` widened to `[:Param, :Field, :Variant]`; HIR-side producer emits edges for all three shapes.
    ///
    /// Paired lockstep `graph-specs-rust` cross-fixture bump per cfdb
    /// CLAUDE.md §3 / RFC-033 §4 I2.
    pub const V0_3_0: Self = Self {
        major: 0,
        minor: 3,
        patch: 0,
    };

    /// **v0.3.1 — RFC-036 §3.3 enrich_metrics producer landing (#203).**
    /// Previously-reserved `EnrichMetrics`-provenance attrs on `:Item`
    /// (`unwrap_count`, `cyclomatic`, `test_coverage`, `dup_cluster_id`)
    /// are now populated by `PetgraphStore::enrich_metrics` when the
    /// `quality-metrics` feature is active. The attrs were described in
    /// V0_3_0 but not emitted; V0_3_1 keyspaces carry real values.
    ///
    /// **Additive and non-breaking within 0.3.x.** V0_3_0 readers
    /// loading a V0_3_1 keyspace see the previously-reserved attrs
    /// populated rather than absent and ignore them (per
    /// `AttributeDescriptor` contract — consumers never rely on absence).
    ///
    /// **G6 invariant:** `test_coverage` is toolchain-version-scoped
    /// (depends on `cargo-llvm-cov` output) and therefore excluded from
    /// the G1 canonical-dump sha256. Documented in `SchemaDescribe`
    /// output so downstream consumers know not to G1-compare across
    /// toolchain bumps.
    ///
    /// Paired lockstep `graph-specs-rust` cross-fixture bump per cfdb
    /// CLAUDE.md §3 / RFC-033 §4 I2.
    pub const V0_3_1: Self = Self {
        major: 0,
        minor: 3,
        patch: 1,
    };

    /// The schema version this build of cfdb-core writes and reads.
    /// Producers tag every keyspace persist with `CURRENT`. Consumers use
    /// `CURRENT.can_read(&file.schema_version)` to reject forward-
    /// incompatible graphs per G4.
    pub const CURRENT: Self = Self::V0_3_1;

    pub fn new(major: u16, minor: u16, patch: u16) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    /// G4: a reader at version `self` can query any graph written at a version
    /// with the same major whose (minor, patch) is less than or equal to self.
    pub fn can_read(&self, graph_version: &Self) -> bool {
        self.major == graph_version.major && graph_version <= self
    }
}

impl fmt::Display for SchemaVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_version_compat() {
        let reader = SchemaVersion::new(0, 1, 0);
        assert!(reader.can_read(&SchemaVersion::new(0, 1, 0)));
        assert!(!reader.can_read(&SchemaVersion::new(0, 1, 1))); // newer minor: no
        assert!(!reader.can_read(&SchemaVersion::new(1, 0, 0))); // different major: no
    }

    // ---- Serde round-trip tests (#3625 AC) ---------------------------------

    #[test]
    fn label_serde_round_trip() {
        let l = Label::new(Label::ITEM);
        let json = serde_json::to_string(&l).expect("Label is a transparent String newtype");
        // #[serde(transparent)] flattens to a bare string.
        assert_eq!(json, "\"Item\"");
        let back: Label = serde_json::from_str(&json).expect("round-trip of just-serialized Label");
        assert_eq!(l, back);
    }

    #[test]
    fn edge_label_serde_round_trip() {
        let e = EdgeLabel::new(EdgeLabel::CALLS);
        let json = serde_json::to_string(&e).expect("EdgeLabel is a transparent String newtype");
        assert_eq!(json, "\"CALLS\"");
        let back: EdgeLabel =
            serde_json::from_str(&json).expect("round-trip of just-serialized EdgeLabel");
        assert_eq!(e, back);
    }

    #[test]
    fn keyspace_serde_round_trip() {
        let k = Keyspace::new("qbot-core");
        let json = serde_json::to_string(&k).expect("Keyspace is a transparent String newtype");
        assert_eq!(json, "\"qbot-core\"");
        let back: Keyspace =
            serde_json::from_str(&json).expect("round-trip of just-serialized Keyspace");
        assert_eq!(k, back);
    }

    #[test]
    fn schema_version_serde_round_trip() {
        let v = SchemaVersion::V0_1_0;
        let json = serde_json::to_string(&v).expect("SchemaVersion has a plain derived Serialize");
        let back: SchemaVersion =
            serde_json::from_str(&json).expect("round-trip of just-serialized SchemaVersion");
        assert_eq!(v, back);
    }
}

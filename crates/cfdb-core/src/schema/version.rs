//! `SchemaVersion` — semantic version of the fact schema in a keyspace.
//!
//! G4 (RFC §6) requires monotonic compatibility within a major. Extracted
//! from `labels.rs` to keep that module cohesive around the label
//! vocabulary and below the 500-line architecture gate (#498).

use std::fmt;

use serde::{Deserialize, Serialize};

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

    /// **v0.3.2 — RFC-040 slice 1/5 schema declaration (#323).** Reserves
    /// the `:ConstTable` node label, the `HAS_CONST_TABLE` edge label, and
    /// the corresponding describer entries. No producer wired yet — first
    /// emissions land in slice 3/5 (issue #325) when the extractor walks
    /// `visit_item_const` and recognizes literal slice/array tables.
    ///
    /// **Additive and non-breaking within 0.3.x.** V0_3_1 readers loading
    /// a V0_3_2 keyspace see no new facts (no producer yet); once slice 3
    /// lands the new nodes / edges appear and V0_3_1 readers ignore the
    /// extra labels.
    ///
    /// Paired lockstep `graph-specs-rust` cross-fixture bump per cfdb
    /// CLAUDE.md §3 / RFC-033 §4 I2 lands in slice 5/5 (issue #327).
    pub const V0_3_2: Self = Self {
        major: 0,
        minor: 3,
        patch: 2,
    };

    /// **v0.4.0 — RFC-041 slice 041-A (#369): `:Literal` fact type.**
    /// Reserves the `:Literal` node label (one node per string literal in
    /// production source) and its describer entry. No producer wired yet —
    /// first emissions land in slice 041-B (issue #370) when the
    /// `cfdb-extractor` `literal_visitor.rs` submodule walks `syn::Lit::Str`
    /// alongside the existing `:CallSite` pass.
    ///
    /// **Additive and non-breaking within major 0 (G4).** A new fact type
    /// is a capability boundary, so this is a minor bump (precedent: every
    /// prior label-introducing epoch — V0_2_0 entry points, V0_3_0
    /// schema-producer alignment). V0_3_2 readers loading a V0_4_0 keyspace
    /// see no new facts (no producer until 041-B); once 041-B lands the new
    /// `:Literal` nodes appear and V0_3_2 readers ignore the extra label.
    ///
    /// Paired lockstep `graph-specs-rust` cross-fixture bump per cfdb
    /// CLAUDE.md §3 / RFC-033 §4 I5 lands in slice 041-D (issue #372) —
    /// NOT this slice.
    pub const V0_4_0: Self = Self {
        major: 0,
        minor: 4,
        patch: 0,
    };

    /// **v0.5.0 — RFC-043 Slice A: `:Argument` fact type + `HAS_ARG` edge.**
    /// Introduces `Label::ARGUMENT`, `EdgeLabel::HAS_ARG`, and the
    /// `argument_node_id` helper. Both syn-extractor (`cfdb-extractor`) and
    /// HIR-extractor (`cfdb-hir-extractor`) emit `:Argument` nodes and
    /// `HAS_ARG` edges for every call expression they visit. A new fact type
    /// (new node label class) warrants a minor bump per the V0_2_0 /
    /// V0_3_0 / V0_4_0 precedent (solid §5.3 BLOCKER 1 / rust-systems §5.4
    /// finding 3). Pre-V0_5_0 keyspaces carry zero `:Argument` nodes.
    ///
    /// **Breaking within 0.x:** V0_4_0 readers refuse V0_5_0 graphs per G4
    /// (`can_read` returns false when graph.minor > reader.minor). This is the
    /// intended signal that V0_5_0 graphs may contain `:Argument` / `HAS_ARG`
    /// facts that V0_4_0 readers do not understand.
    ///
    /// Paired lockstep `graph-specs-rust` cross-fixture bump per cfdb
    /// CLAUDE.md §3 / RFC-033 §4 I5 lands in Slice B (issue #443).
    pub const V0_5_0: Self = Self {
        major: 0,
        minor: 5,
        patch: 0,
    };

    /// **v0.6.0 — RFC-050 Slice 50-A: `:Crate.crate_tier` attribute.**
    /// Adds a single additive `:Crate.crate_tier` (int) attribute carrying
    /// each crate's topological longest-path depth in the intra-workspace
    /// normal-`[dependencies]` DAG (leaf = 0; `tier = 1 + max(dep tiers)`),
    /// computed at extract time with `Provenance::Extractor`. No new node
    /// or edge label, no new verb. A new extractor fact warrants a minor
    /// bump per the V0_2_0 / V0_5_0 precedent.
    ///
    /// **Breaking within 0.x:** V0_5_0 readers refuse V0_6_0 graphs per G4
    /// (`can_read` returns false when graph.minor > reader.minor) — the
    /// intended signal that V0_6_0 graphs carry a `crate_tier` attribute
    /// V0_5_0 readers do not understand. Pre-V0_6_0 keyspaces carry no
    /// `crate_tier` on any `:Crate`.
    ///
    /// Paired lockstep `graph-specs-rust` cross-fixture bump per cfdb
    /// CLAUDE.md §3 / RFC-033 §4 I5.
    pub const V0_6_0: Self = Self {
        major: 0,
        minor: 6,
        patch: 0,
    };

    /// **v0.7.0 — RFC-053 slice 53-A: `:MatchSite` node + `MATCHES_AT` /
    /// `MATCHES_ON` edge labels.** Introduces `Label::MATCH_SITE` (one node
    /// per `match` expression × distinct name-level matched-path prefix,
    /// emitted by the `cfdb-extractor` `match_visitor` as a third
    /// independent per-fn-body pass), the walk-time `EdgeLabel::MATCHES_AT`
    /// (`:Item` → `:MatchSite`), and the reserved `EdgeLabel::MATCHES_ON`
    /// (`:MatchSite` → `:Item{kind:"enum"}`; producer lands in slice 53-B).
    /// A new node label class + edge vocabulary warrants a minor bump per
    /// the V0_2_0 / V0_4_0 / V0_5_0 precedent. One bump total for RFC-053;
    /// 53-B and 53-C add no schema surface.
    ///
    /// **Breaking within 0.x:** V0_6_0 readers refuse V0_7_0 graphs per G4
    /// (`can_read` returns false when graph.minor > reader.minor) — the
    /// intended signal that V0_7_0 graphs may carry `:MatchSite` /
    /// `MATCHES_AT` facts V0_6_0 readers do not understand. Pre-V0_7_0
    /// keyspaces carry zero `:MatchSite` nodes (same compat language as
    /// `:Literal`).
    ///
    /// Paired lockstep `graph-specs-rust` cross-fixture bump per cfdb
    /// CLAUDE.md §3 / RFC-033 §4 I2 — merge cfdb first (53-A).
    pub const V0_7_0: Self = Self {
        major: 0,
        minor: 7,
        patch: 0,
    };

    /// The schema version this build of cfdb-core writes and reads.
    /// Producers tag every keyspace persist with `CURRENT`. Consumers use
    /// `CURRENT.can_read(&file.schema_version)` to reject forward-
    /// incompatible graphs per G4.
    pub const CURRENT: Self = Self::V0_7_0;

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

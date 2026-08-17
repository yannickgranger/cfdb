//! `SchemaVersion` — semantic version of the fact schema in a keyspace.
//!
//! Monotonic compatibility within a major version.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Semantic version of the fact schema in a keyspace.
/// Monotonic compatibility within a major — v1.1 graphs are queryable by v1.0
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

    /// v0.1.1 — adds `:Item.visibility` (additive, non-breaking
    /// within the 0.x major).
    pub const V0_1_1: Self = Self {
        major: 0,
        minor: 1,
        patch: 1,
    };

    /// v0.1.2 — adds the optional `:Item.cfg_gate` attribute
    /// carrying the `#[cfg(feature = "…")]` expression tree captured on
    /// the item (absent when the item has no feature-cfg).
    pub const V0_1_2: Self = Self {
        major: 0,
        minor: 1,
        patch: 2,
    };

    /// v0.1.3 — adds the `:CallSite.resolver` and
    /// `:CallSite.callee_resolved` discriminator properties so the syn
    /// (unresolved) and HIR (resolved) extractors can both emit
    /// `:CallSite` without homonym ambiguity.
    /// `cfdb-extractor`-emitted `:CallSite` carries `resolver="syn"`;
    /// `cfdb-hir-extractor`-emitted `:CallSite` carries `resolver="hir"`.
    pub const V0_1_3: Self = Self {
        major: 0,
        minor: 1,
        patch: 3,
    };

    /// v0.1.4 — adds the `CALLS.resolved: bool` edge
    /// attribute distinguishing HIR-resolved dispatch from syn-based textual
    /// calls. Wires resolved `CALLS` + `INVOKES_AT` through the
    /// `cfdb-hir-petgraph-adapter`.
    pub const V0_1_4: Self = Self {
        major: 0,
        minor: 1,
        patch: 4,
    };

    /// v0.2.0 — First emissions of `:EntryPoint` nodes and
    /// `EXPOSES` edges. Introduces the `cfdb-cli --features hir`
    /// composition seam. Minor bump marks the v0.2 capability boundary.
    pub const V0_2_0: Self = Self {
        major: 0,
        minor: 2,
        patch: 0,
    };

    /// v0.2.1 — adds extractor-time emissions of `:Item.is_deprecated` and
    /// `:Item.deprecation_since`. Additive and non-breaking within 0.2.x.
    pub const V0_2_1: Self = Self {
        major: 0,
        minor: 2,
        patch: 1,
    };

    /// v0.2.2 — adds first emissions of impl-block `:Item` nodes plus
    /// `IMPLEMENTS` and `IMPLEMENTS_FOR` edges. Additive and non-breaking
    /// within 0.2.x.
    pub const V0_2_2: Self = Self {
        major: 0,
        minor: 2,
        patch: 2,
    };

    /// v0.2.3 — adds first emissions of `:RfcDoc` nodes and
    /// `(:Item)-[:REFERENCED_BY]->(:RfcDoc)` edges. Additive and
    /// non-breaking within 0.2.x.
    pub const V0_2_3: Self = Self {
        major: 0,
        minor: 2,
        patch: 3,
    };

    /// **v0.3.0 — schema-producer alignment epoch.** Breaking changes:
    /// `:Field.type_qname` prop REMOVED; `EdgeLabel::SUPERTRAIT` +
    /// `EdgeLabel::RECEIVES_ARG` constants REMOVED.
    /// Additive: `:Variant` nodes + `HAS_VARIANT` edges; `:Field` tuple
    /// variants; widened `HAS_FIELD.from` / `REGISTERS_PARAM.to`;
    /// `index`, `type_normalized`, `type_path` attrs; `RETURNS` and
    /// `TYPE_OF` edges.
    pub const V0_3_0: Self = Self {
        major: 0,
        minor: 3,
        patch: 0,
    };

    /// **v0.3.1 — enrich_metrics producer.** Previously-reserved attrs on
    /// `:Item` (`unwrap_count`, `cyclomatic`, `test_coverage`, `dup_cluster_id`)
    /// are now populated when the `quality-metrics` feature is active. The
    /// attrs were described in V0_3_0 but not emitted; V0_3_1 keyspaces carry
    /// real values.
    ///
    /// Additive and non-breaking within 0.3.x — V0_3_0 readers loading a
    /// V0_3_1 keyspace see the previously-reserved attrs populated rather
    /// than absent, and ignore them.
    pub const V0_3_1: Self = Self {
        major: 0,
        minor: 3,
        patch: 1,
    };

    /// **v0.3.2 — schema declaration.** Reserves
    /// the `:ConstTable` node label and `HAS_CONST_TABLE` edge label.
    /// Additive and non-breaking within 0.3.x.
    pub const V0_3_2: Self = Self {
        major: 0,
        minor: 3,
        patch: 2,
    };

    /// **v0.4.0 — `:Literal` fact type.**
    /// Reserves the `:Literal` node label.
    /// Additive and non-breaking within major 0.
    pub const V0_4_0: Self = Self {
        major: 0,
        minor: 4,
        patch: 0,
    };

    /// **v0.5.0 — `:Argument` fact type + `HAS_ARG` edge.**
    /// Both syn-extractor and HIR-extractor emit `:Argument` nodes and
    /// `HAS_ARG` edges for every call expression. Breaking within 0.x.
    pub const V0_5_0: Self = Self {
        major: 0,
        minor: 5,
        patch: 0,
    };

    /// **v0.6.0 — `:Crate.crate_tier` attribute.**
    /// Each crate carries topological longest-path depth in the intra-workspace
    /// normal-`[dependencies]` DAG. Breaking within 0.x.
    pub const V0_6_0: Self = Self {
        major: 0,
        minor: 6,
        patch: 0,
    };

    /// **v0.7.0 — `:MatchSite` node + `MATCHES_AT` / `MATCHES_ON` edges.**
    /// One node per `match` expression × distinct name-level matched-path
    /// prefix. Breaking within 0.x.
    pub const V0_7_0: Self = Self {
        major: 0,
        minor: 7,
        patch: 0,
    };

    /// **V0_8_0 — target-scoped `:Item` identity + the `:Item.target` attribute.**
    /// New `:Item.target` attribute (`"lib"` / `"bin:<target-name>"`).
    /// Bin-target items gain a `#bin:{target}` identity suffix so distinct
    /// cargo targets stop colliding on one node. Breaking within 0.x.
    pub const V0_8_0: Self = Self {
        major: 0,
        minor: 8,
        patch: 0,
    };

    /// The schema version this build of cfdb-core writes and reads.
    pub const CURRENT: Self = Self::V0_8_0;

    pub fn new(major: u16, minor: u16, patch: u16) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    /// A reader at version `self` can query any graph written at a version
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

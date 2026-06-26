//! Label newtypes — node/edge labels, keyspace, schema version.
//!
//! RFC §7 defines the ten node labels and ~20 edge labels. This module encodes
//! them as plain strings wrapped in newtypes so the extractor, parser, and
//! evaluator can share a single vocabulary without stringly-typing it.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Named convention constant for the method-call receiver position (RFC-043
/// §3.1 / solid §5.3 finding 4). For `ExprMethodCall`, the implicit `self`
/// argument is at position 0; positional args follow from 1.
/// Cypher rule authors reference this conceptually as the stable anchor
/// for receiver-type fence predicates (`WHERE arg.position = 0`).
pub const RECEIVER_POSITION: u32 = 0;

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
    /// A literal const slice/array recognized by the extractor as a "table"
    /// of values (RFC-040). Carries `qname`, `name`, `crate`, `module_qpath`,
    /// `element_type` (closed-set wire string ∈ `{"str", "u32", "i32",
    /// "u64", "i64"}`), `entry_count`, `entries_hash`, `entries_normalized`,
    /// `entries_sample`, `is_test`. Reserved in slice 1/5 (issue #323);
    /// first emissions land in slice 3/5 (issue #325). The
    /// `(:Item) -[:HAS_CONST_TABLE]-> (:ConstTable)` edge encodes parent →
    /// satellite ownership matching the established `HAS_*` family.
    pub const CONST_TABLE: &'static str = "ConstTable";
    /// A single string literal occurring in production source
    /// (`crates/*/src/**/*.rs`), modelled at the `:CallSite` abstraction
    /// level (RFC-041). Carries `value` (raw inter-delimiter source bytes,
    /// NOT `syn::LitStr::value()` — the `=~`-matches-`grep` invariant,
    /// RFC-041 §3.1), `file`, `line`, `col`, `crate`, `is_test`. Node id is
    /// `literal:<workspace-relative-file>:<line>:<col>` (collision-free by
    /// Rust grammar; `:Literal` has no owning `:Item` in v0). Deliberately
    /// NO `kind` attr — that is a three-way homonym vs `:Item.kind` and
    /// `:ConstTable.element_type`; future non-string literals use
    /// `lit_syntax`, not `kind` (ddd lens, council 2026-05-15). Reserved in
    /// slice 041-A (issue #369); first emissions land in slice 041-B
    /// (issue #370) via the `cfdb-extractor` `literal_visitor.rs` submodule.
    /// Pre-V0_4_0 keyspaces carry zero `:Literal` nodes.
    pub const LITERAL: &'static str = "Literal";
    /// A positional argument at a call site (RFC-043 Slice A). Carries
    /// `source_text`, `kind`, `position`, `file`, `line`, `col`. Connected
    /// from its owning `:CallSite` via `[:HAS_ARG]`. Node id is
    /// `arg:{callsite_id}#{position}` (derived via
    /// `cfdb_core::qname::argument_node_id`). SchemaVersion V0_5_0+.
    pub const ARGUMENT: &'static str = "Argument";

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
    /// `(:Item{kind="const"}) -[:HAS_CONST_TABLE]-> (:ConstTable)`. Reserved
    /// in slice 1/5 (issue #323) per RFC-040 §3.2; first emissions land in
    /// slice 3/5 (issue #325). Direction is parent → satellite, matching
    /// the rest of the `HAS_*` family.
    pub const HAS_CONST_TABLE: &'static str = "HAS_CONST_TABLE";
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
    /// `(:CallSite)-[:HAS_ARG]->(:Argument)` — connects a call site to each
    /// of its positional arguments (RFC-043 Slice A). Direction is call site →
    /// argument. Position lives on the `:Argument` node (not on this edge) per
    /// DDD §5.2 NIT, mirroring `:Param.index` / `:Field.index` / `:Variant.index`.
    /// SchemaVersion V0_5_0+.
    pub const HAS_ARG: &'static str = "HAS_ARG";

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

#[cfg(test)]
mod tests;

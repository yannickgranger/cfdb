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
    pub const CALL_SITE: &'static str = "CallSite";
    pub const ENTRY_POINT: &'static str = "EntryPoint";
    pub const CONCEPT: &'static str = "Concept";
    pub const CONTEXT: &'static str = "Context";

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
    pub const SUPERTRAIT: &'static str = "SUPERTRAIT";
    pub const BELONGS_TO: &'static str = "BELONGS_TO";

    // Call graph
    pub const CALLS: &'static str = "CALLS";
    pub const INVOKES_AT: &'static str = "INVOKES_AT";
    pub const RECEIVES_ARG: &'static str = "RECEIVES_ARG";

    // Entry points
    pub const EXPOSES: &'static str = "EXPOSES";
    pub const REGISTERS_PARAM: &'static str = "REGISTERS_PARAM";

    // Concept overlay
    pub const LABELED_AS: &'static str = "LABELED_AS";
    pub const CANONICAL_FOR: &'static str = "CANONICAL_FOR";
    pub const EQUIVALENT_TO: &'static str = "EQUIVALENT_TO";

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

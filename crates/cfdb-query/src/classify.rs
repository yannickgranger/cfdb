//! Wire envelope for `cfdb classify` — debt-class routing of diff-restricted
//! findings.
//!
//! Composed of a [`ScopeInventory`] (the #48 classifier output) plus a
//! [`DiffSourceMeta`] block that identifies the upstream `cfdb diff`
//! envelope. Consumers (qbot-core #3736 per-PR drift gate,
//! `/operate-module`, `/boy-scout --from-inventory`) deserialise this type
//! directly — routing from `DebtClass` → concrete Claude skill happens
//! via the external `SkillRoutingTable` (RFC-cfdb.md §A2.3), not on the
//! finding rows (enforced by `finding_no_skill_field`).
//!
//! # Envelope schema versioning
//!
//! [`ClassifyEnvelope::schema_version`] is pinned to
//! [`CLASSIFY_ENVELOPE_SCHEMA_VERSION`] and versions the wire shape of
//! this envelope only — NOT `cfdb_core::SchemaVersion` (on-disk
//! keyspaces) and NOT the diff envelope's [`crate::diff::ENVELOPE_SCHEMA_VERSION`]
//! (which evolves independently).

use serde::{Deserialize, Serialize};

use crate::inventory::ScopeInventory;

/// Envelope schema version. Bumped only when the `ClassifyEnvelope` wire
/// shape changes in a breaking way.
pub const CLASSIFY_ENVELOPE_SCHEMA_VERSION: &str = "v1";

/// Wire envelope for `cfdb classify`. Composition of a `ScopeInventory`
/// (all 6 `DebtClass` buckets, warnings) and `DiffSourceMeta` (the
/// upstream diff identity that drove the restriction).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ClassifyEnvelope {
    /// Envelope schema version — always [`CLASSIFY_ENVELOPE_SCHEMA_VERSION`].
    pub schema_version: String,
    /// Classifier output restricted to the diff's `added` ∪ `changed`
    /// qnames. Same shape #48 shipped — consumers that already deserialise
    /// `ScopeInventory` (e.g. `cfdb scope`) share the bucket layout.
    pub inventory: ScopeInventory,
    /// Upstream diff identity — `(a, b)` keyspace pair + count of qnames
    /// that survived the restriction. Does NOT embed the raw diff envelope;
    /// consumers that need the full delta consume `cfdb diff` separately.
    pub diff_source: DiffSourceMeta,
}

impl ClassifyEnvelope {
    /// Construct an envelope with the pinned schema version.
    pub fn new(inventory: ScopeInventory, diff_source: DiffSourceMeta) -> Self {
        Self {
            schema_version: CLASSIFY_ENVELOPE_SCHEMA_VERSION.to_string(),
            inventory,
            diff_source,
        }
    }
}

/// Identity of the upstream `cfdb diff` envelope that drove the
/// classification. Projection of `DiffEnvelope.{a, b}` plus a
/// `restrict_count` — how many distinct qnames the handler pulled out
/// of the diff envelope's `added` ∪ `changed` facts to use as the
/// restrict set.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DiffSourceMeta {
    /// Left keyspace name from the source diff (the "before").
    pub a: String,
    /// Right keyspace name from the source diff (the "after").
    pub b: String,
    /// Cardinality of the restrict set derived from the diff (number of
    /// distinct qnames across `added` ∪ `changed`).
    pub restrict_count: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_inventory() -> ScopeInventory {
        ScopeInventory::new("ctx", "aabbccddeeff")
    }

    #[test]
    fn envelope_new_pins_schema_version() {
        let env = ClassifyEnvelope::new(
            empty_inventory(),
            DiffSourceMeta {
                a: "cfdb-prev".into(),
                b: "cfdb".into(),
                restrict_count: 0,
            },
        );
        assert_eq!(env.schema_version, CLASSIFY_ENVELOPE_SCHEMA_VERSION);
        assert_eq!(env.diff_source.a, "cfdb-prev");
        assert_eq!(env.diff_source.b, "cfdb");
    }

    #[test]
    fn envelope_serde_round_trip() {
        let original = ClassifyEnvelope::new(
            empty_inventory(),
            DiffSourceMeta {
                a: "a".into(),
                b: "b".into(),
                restrict_count: 42,
            },
        );
        let serialised = serde_json::to_string(&original).unwrap();
        let back: ClassifyEnvelope = serde_json::from_str(&serialised).unwrap();
        assert_eq!(original, back);
    }

    #[test]
    fn envelope_json_does_not_leak_skill_fields() {
        let env = ClassifyEnvelope::new(
            empty_inventory(),
            DiffSourceMeta {
                a: "a".into(),
                b: "b".into(),
                restrict_count: 0,
            },
        );
        let json = serde_json::to_string(&env).unwrap();
        // Same-shape invariant as the `finding_no_skill_field` arch test,
        // applied to the composed envelope (#213 forbidden move #1).
        for banned in [
            "fix_skill",
            "skill_name",
            "skill_route",
            "routing",
            "council_required",
            "concrete_skill",
        ] {
            assert!(
                !json.contains(banned),
                "ClassifyEnvelope JSON MUST NOT contain `{banned}` — \
                 routing is external via .cfdb/skill-routing.toml"
            );
        }
    }

    #[test]
    fn diff_source_meta_serde_keys_are_snake_case() {
        let meta = DiffSourceMeta {
            a: "a".into(),
            b: "b".into(),
            restrict_count: 7,
        };
        let json = serde_json::to_string(&meta).unwrap();
        assert!(json.contains("\"restrict_count\":7"));
        assert!(json.contains("\"a\":\"a\""));
        assert!(json.contains("\"b\":\"b\""));
    }
}

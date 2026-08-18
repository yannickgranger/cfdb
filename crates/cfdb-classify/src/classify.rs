//! Wire envelope for `cfdb classify` — debt-class routing of diff-restricted
//! findings.
//!
//! Composed of a [`ScopeInventory`] (classifier output) plus a
//! [`DiffSourceMeta`] block that identifies the upstream `cfdb diff`
//! envelope. Consumers deserialise this type directly — which skill handles
//! a `DebtClass` is the consumer's decision, never a field on the finding
//! rows (enforced by `finding_no_skill_field`).
//!
//! # Envelope schema versioning
//!
//! [`ClassifyEnvelope::schema_version`] is pinned to
//! [`CLASSIFY_ENVELOPE_SCHEMA_VERSION`] and versions the wire shape of
//! this envelope only — NOT `cfdb_core::SchemaVersion` (on-disk
//! keyspaces) and NOT the diff envelope's [`cfdb_query::diff::ENVELOPE_SCHEMA_VERSION`]
//! (which evolves independently).

use std::collections::BTreeSet;

use cfdb_query::DiffEnvelope;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::taxonomy::{DebtClass, Finding, ScopeInventory};

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
    /// qnames. Same shape as existing scope output — consumers that already
    /// deserialise `ScopeInventory` (e.g. `cfdb scope`) share the bucket layout.
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

/// Derive the restrict-qname set from a `DiffEnvelope`. Includes every
/// node qname (props.qname with id fallback) from `added` and the two
/// envelope sides of `changed`, plus edge endpoint qnames (src_qname +
/// dst_qname) so classifier findings whose `:Item.qname` equals an edge
/// endpoint on a changed relationship are retained.
pub(crate) fn collect_restrict_qnames(env: &DiffEnvelope) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for fact in &env.added {
        extend_with_envelope_qnames(&mut out, &fact.envelope);
    }
    for fact in &env.changed {
        extend_with_envelope_qnames(&mut out, &fact.a);
        extend_with_envelope_qnames(&mut out, &fact.b);
    }
    out
}

fn extend_with_envelope_qnames(out: &mut BTreeSet<String>, envelope: &Value) {
    if let Some(kind) = envelope.get("kind").and_then(Value::as_str) {
        match kind {
            "node" => {
                // Prefer props.qname; fall back to id (matches
                // canonical_dump's sort-key resolution).
                if let Some(q) = envelope
                    .get("props")
                    .and_then(|p| p.get("qname"))
                    .and_then(Value::as_str)
                {
                    out.insert(q.to_string());
                } else if let Some(id) = envelope.get("id").and_then(Value::as_str) {
                    out.insert(id.to_string());
                }
            }
            "edge" => {
                if let Some(src) = envelope.get("src_qname").and_then(Value::as_str) {
                    out.insert(src.to_string());
                }
                if let Some(dst) = envelope.get("dst_qname").and_then(Value::as_str) {
                    out.insert(dst.to_string());
                }
            }
            _ => {}
        }
    }
}

impl ClassifyEnvelope {
    /// Every finding in the envelope in the pinned emission order:
    /// classes by `DebtClass::Ord`, findings within a class by `Finding::Ord`.
    /// The sorted-JSONL renderer and any consumer that wants a stable row
    /// order read this — the order is a contract, not a presentation choice.
    pub fn sorted_rows(&self) -> Vec<(DebtClass, &Finding)> {
        let mut out = Vec::new();
        for (class, class_findings) in &self.inventory.findings_by_class {
            let mut findings: Vec<&Finding> = class_findings.iter().collect();
            findings.sort();
            out.extend(findings.into_iter().map(|f| (*class, f)));
        }
        out
    }
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
                 skill routing is the consumer's concern, external to cfdb"
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

#[cfg(test)]
mod restrict_tests {
    use super::*;
    use cfdb_query::{ChangedFact, DiffFact, ENVELOPE_SCHEMA_VERSION};
    use serde_json::json;

    fn node_envelope(qname: &str) -> Value {
        json!({
            "id": format!("item:{qname}"),
            "kind": "node",
            "label": "Item",
            "props": {"qname": qname},
        })
    }

    fn edge_envelope(src: &str, dst: &str) -> Value {
        json!({
            "dst_qname": dst,
            "kind": "edge",
            "label": "CALLS",
            "props": {},
            "src_qname": src,
        })
    }

    fn empty_diff(a: &str, b: &str) -> DiffEnvelope {
        DiffEnvelope {
            a: a.into(),
            b: b.into(),
            schema_version: ENVELOPE_SCHEMA_VERSION.into(),
            added: vec![],
            removed: vec![],
            changed: vec![],
            warnings: vec![],
        }
    }

    #[test]
    fn collect_restrict_qnames_extracts_added_node_props_qname() {
        let mut env = empty_diff("a", "b");
        env.added.push(DiffFact {
            kind: "node".into(),
            envelope: node_envelope("foo::Bar"),
        });
        let set = collect_restrict_qnames(&env);
        assert!(set.contains("foo::Bar"));
        assert_eq!(set.len(), 1);
    }

    #[test]
    fn collect_restrict_qnames_falls_back_to_id_when_props_qname_absent() {
        let mut env = empty_diff("a", "b");
        env.added.push(DiffFact {
            kind: "node".into(),
            envelope: json!({
                "id": "callsite:abc123",
                "kind": "node",
                "label": "CallSite",
                "props": {},
            }),
        });
        let set = collect_restrict_qnames(&env);
        assert!(set.contains("callsite:abc123"));
    }

    #[test]
    fn collect_restrict_qnames_includes_edge_endpoints() {
        let mut env = empty_diff("a", "b");
        env.added.push(DiffFact {
            kind: "edge".into(),
            envelope: edge_envelope("caller::fn", "callee::fn"),
        });
        let set = collect_restrict_qnames(&env);
        assert!(set.contains("caller::fn"));
        assert!(set.contains("callee::fn"));
    }

    #[test]
    fn collect_restrict_qnames_includes_both_sides_of_changed() {
        let mut env = empty_diff("a", "b");
        env.changed.push(ChangedFact {
            kind: "node".into(),
            a: node_envelope("old::Name"),
            b: node_envelope("new::Name"),
        });
        let set = collect_restrict_qnames(&env);
        assert!(set.contains("old::Name"));
        assert!(set.contains("new::Name"));
    }

    #[test]
    fn collect_restrict_qnames_unions_added_and_changed() {
        let mut env = empty_diff("a", "b");
        env.added.push(DiffFact {
            kind: "node".into(),
            envelope: node_envelope("added::X"),
        });
        env.changed.push(ChangedFact {
            kind: "node".into(),
            a: node_envelope("changed::Y"),
            b: node_envelope("changed::Y"),
        });
        env.added.push(DiffFact {
            kind: "edge".into(),
            envelope: edge_envelope("edge::src", "edge::dst"),
        });
        let set = collect_restrict_qnames(&env);
        assert_eq!(set.len(), 4);
        for q in ["added::X", "changed::Y", "edge::src", "edge::dst"] {
            assert!(set.contains(q), "missing {q}");
        }
    }

    #[test]
    fn collect_restrict_qnames_empty_diff_produces_empty_set() {
        let env = empty_diff("a", "b");
        assert!(collect_restrict_qnames(&env).is_empty());
    }
}

//! RFC-034 §4.2 frozen JSON envelope.
//!
//! The shape is OCP-locked at `schema_version = "v1"`:
//!
//! ```json
//! {
//!   "schema_version": "v1",
//!   "from_ref": "<sha>",
//!   "to_ref": "<sha>",
//!   "triggers_fired": ["C1", "C3"],
//!   "evidence": {
//!     "C1": { "contexts_touched": ["trading", "risk"] },
//!     "C3": { "matched_paths": ["crates/ports-trading/src/foo.rs"] }
//!   }
//! }
//! ```
//!
//! Future Tier-2 promotions append IDs to `triggers_fired` + evidence keys —
//! they do NOT add per-trigger boolean fields (Forbidden move #3).

use serde::Serialize;
use std::collections::BTreeMap;

use crate::trigger_id::TriggerId;

/// Frozen envelope schema identifier. Bumping this string is a BREAKING change
/// that every consumer skill must opt into; additive trigger changes keep `v1`.
pub const SCHEMA_VERSION: &str = "v1";

/// RFC-034 §4.2 envelope emitted on stdout.
#[derive(Debug, Serialize)]
pub struct PreludeTriggerReport {
    pub schema_version: &'static str,
    pub from_ref: String,
    pub to_ref: String,
    /// Sorted, deduped list of fired trigger IDs. Empty when no trigger fired.
    pub triggers_fired: Vec<TriggerId>,
    /// Per-trigger evidence keyed by string form (`"C1"`, `"C3"`, ...).
    /// Only fired triggers appear as keys.
    pub evidence: BTreeMap<String, serde_json::Value>,
}

impl PreludeTriggerReport {
    /// Build an empty report for the given ref range. Handlers mutate
    /// `triggers_fired` + `evidence` as they run.
    #[must_use]
    pub fn new(from_ref: String, to_ref: String) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            from_ref,
            to_ref,
            triggers_fired: Vec::new(),
            evidence: BTreeMap::new(),
        }
    }

    /// Record a trigger firing with its evidence payload.
    pub fn record(&mut self, id: TriggerId, evidence: serde_json::Value) {
        if !self.triggers_fired.contains(&id) {
            self.triggers_fired.push(id);
            self.triggers_fired.sort();
        }
        self.evidence.insert(id.as_str().to_string(), evidence);
    }
}

#[cfg(test)]
mod tests {
    use super::{PreludeTriggerReport, SCHEMA_VERSION};
    use crate::trigger_id::TriggerId;
    use serde_json::json;

    #[test]
    fn schema_version_is_frozen_v1() {
        assert_eq!(SCHEMA_VERSION, "v1");
        let r = PreludeTriggerReport::new("a".into(), "b".into());
        assert_eq!(r.schema_version, "v1");
    }

    #[test]
    fn record_populates_triggers_fired_and_evidence() {
        let mut r = PreludeTriggerReport::new("a".into(), "b".into());
        r.record(TriggerId::C3, json!({"matched_paths": ["x"]}));
        r.record(TriggerId::C1, json!({"contexts_touched": ["y", "z"]}));
        // deduplication + sort
        r.record(TriggerId::C1, json!({"contexts_touched": ["y", "z"]}));
        assert_eq!(r.triggers_fired, vec![TriggerId::C1, TriggerId::C3]);
        assert!(r.evidence.contains_key("C1"));
        assert!(r.evidence.contains_key("C3"));
    }

    #[test]
    fn serializes_without_per_trigger_boolean_fields() {
        let mut r = PreludeTriggerReport::new("a".into(), "b".into());
        r.record(
            TriggerId::C1,
            json!({"contexts_touched": ["trading", "risk"]}),
        );
        let s = serde_json::to_string(&r).expect("serialize");
        // OCP lock: no per-trigger boolean sibling fields.
        assert!(!s.contains("\"c1\":"), "forbidden per-trigger boolean: {s}");
        assert!(!s.contains("\"c3\":"), "forbidden per-trigger boolean: {s}");
        assert!(s.contains("\"triggers_fired\":[\"C1\"]"));
        assert!(s.contains("\"schema_version\":\"v1\""));
    }
}

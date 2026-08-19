use serde::Serialize;
use std::collections::BTreeMap;

use crate::trigger_id::TriggerId;

pub const SCHEMA_VERSION: &str = "v1";

#[derive(Debug, Serialize)]
pub struct PreludeTriggerReport {
    pub schema_version: &'static str,
    pub from_ref: String,
    pub to_ref: String,
    pub triggers_fired: Vec<TriggerId>,
    pub evidence: BTreeMap<String, serde_json::Value>,
}

impl PreludeTriggerReport {
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
        assert!(!s.contains("\"c1\":"), "forbidden per-trigger boolean: {s}");
        assert!(!s.contains("\"c3\":"), "forbidden per-trigger boolean: {s}");
        assert!(s.contains("\"triggers_fired\":[\"C1\"]"));
        assert!(s.contains("\"schema_version\":\"v1\""));
    }
}

use serde::{Deserialize, Serialize};

use crate::schema::Keyspace;
use crate::store::StoreError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnrichReport {
    pub verb: String,
    pub ran: bool,
    pub facts_scanned: u64,
    pub attrs_written: u64,
    pub edges_written: u64,
    pub warnings: Vec<String>,
}

impl EnrichReport {
    pub fn not_implemented(verb: &str) -> Self {
        Self {
            verb: verb.to_string(),
            ran: false,
            facts_scanned: 0,
            attrs_written: 0,
            edges_written: 0,
            warnings: vec![format!(
                "{verb}: enrichment pass not implemented in v0.1 (deferred to v0.2 / Phase D — EPIC #3622)"
            )],
        }
    }

    pub fn is_complete(&self) -> bool {
        self.ran
    }
}

pub trait EnrichBackend: Send + Sync {
    fn enrich_git_history(&mut self, _keyspace: &Keyspace) -> Result<EnrichReport, StoreError> {
        Ok(EnrichReport::not_implemented("enrich_git_history"))
    }

    fn enrich_rfc_docs(&mut self, _keyspace: &Keyspace) -> Result<EnrichReport, StoreError> {
        Ok(EnrichReport::not_implemented("enrich_rfc_docs"))
    }

    fn enrich_deprecation(&mut self, _keyspace: &Keyspace) -> Result<EnrichReport, StoreError> {
        Ok(EnrichReport::not_implemented("enrich_deprecation"))
    }

    fn enrich_bounded_context(&mut self, _keyspace: &Keyspace) -> Result<EnrichReport, StoreError> {
        Ok(EnrichReport::not_implemented("enrich_bounded_context"))
    }

    fn enrich_concepts(&mut self, _keyspace: &Keyspace) -> Result<EnrichReport, StoreError> {
        Ok(EnrichReport::not_implemented("enrich_concepts"))
    }

    fn enrich_reachability(&mut self, _keyspace: &Keyspace) -> Result<EnrichReport, StoreError> {
        Ok(EnrichReport::not_implemented("enrich_reachability"))
    }

    fn enrich_metrics(&mut self, _keyspace: &Keyspace) -> Result<EnrichReport, StoreError> {
        Ok(EnrichReport::not_implemented("enrich_metrics"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn not_implemented_marks_pass_as_unran() {
        let r = EnrichReport::not_implemented("enrich_docs");
        assert_eq!(r.verb, "enrich_docs");
        assert!(!r.ran);
        assert!(!r.is_complete());
        assert_eq!(r.facts_scanned, 0);
        assert_eq!(r.attrs_written, 0);
        assert_eq!(r.edges_written, 0);
        assert_eq!(r.warnings.len(), 1);
        assert!(r.warnings[0].contains("enrich_docs"));
        assert!(r.warnings[0].contains("v0.2"));
    }

    #[test]
    fn not_implemented_warning_mentions_phase_d() {
        let r = EnrichReport::not_implemented("enrich_metrics");
        assert!(
            r.warnings[0].contains("Phase D"),
            "stub warning must point at Phase D so callers can grep for it"
        );
    }

    #[test]
    fn report_round_trips_through_serde() {
        let original = EnrichReport {
            verb: "enrich_history".to_string(),
            ran: true,
            facts_scanned: 1234,
            attrs_written: 56,
            edges_written: 7,
            warnings: vec!["partial: 3 commits unreadable".to_string()],
        };
        let json = serde_json::to_string(&original).expect("serialize");
        let decoded: EnrichReport = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded, original);
    }

    #[test]
    fn is_complete_reflects_ran_flag() {
        let stub = EnrichReport::not_implemented("enrich_concepts");
        assert!(!stub.is_complete());

        let real = EnrichReport {
            verb: "enrich_concepts".to_string(),
            ran: true,
            facts_scanned: 10,
            attrs_written: 2,
            edges_written: 0,
            warnings: vec![],
        };
        assert!(real.is_complete());
    }

    struct TestBackend;
    impl EnrichBackend for TestBackend {}

    fn call_and_assert_not_implemented(report: EnrichReport, expected_verb: &'static str) {
        assert_eq!(report.verb, expected_verb);
        assert!(!report.ran);
        assert_eq!(report.facts_scanned, 0);
        assert_eq!(report.attrs_written, 0);
        assert_eq!(report.edges_written, 0);
        assert_eq!(report.warnings.len(), 1);
        assert!(
            report.warnings[0].contains(expected_verb),
            "warning for `{expected_verb}` must name the verb: {:?}",
            report.warnings[0]
        );
    }

    #[test]
    fn enrich_git_history_default_stub_returns_not_implemented() {
        let ks = Keyspace::new("test");
        let mut b = TestBackend;
        let r = b
            .enrich_git_history(&ks)
            .expect("default stub is infallible");
        call_and_assert_not_implemented(r, "enrich_git_history");
    }

    #[test]
    fn enrich_rfc_docs_default_stub_returns_not_implemented() {
        let ks = Keyspace::new("test");
        let mut b = TestBackend;
        let r = b.enrich_rfc_docs(&ks).expect("default stub is infallible");
        call_and_assert_not_implemented(r, "enrich_rfc_docs");
    }

    #[test]
    fn enrich_bounded_context_default_stub_returns_not_implemented() {
        let ks = Keyspace::new("test");
        let mut b = TestBackend;
        let r = b
            .enrich_bounded_context(&ks)
            .expect("default stub is infallible");
        call_and_assert_not_implemented(r, "enrich_bounded_context");
    }

    #[test]
    fn enrich_deprecation_default_stub_returns_not_implemented() {
        let ks = Keyspace::new("test");
        let mut b = TestBackend;
        let r = b
            .enrich_deprecation(&ks)
            .expect("default stub is infallible");
        call_and_assert_not_implemented(r, "enrich_deprecation");
    }

    #[test]
    fn enrich_reachability_default_stub_returns_not_implemented() {
        let ks = Keyspace::new("test");
        let mut b = TestBackend;
        let r = b
            .enrich_reachability(&ks)
            .expect("default stub is infallible");
        call_and_assert_not_implemented(r, "enrich_reachability");
    }

    #[test]
    fn enrich_concepts_default_stub_returns_not_implemented() {
        let ks = Keyspace::new("test");
        let mut b = TestBackend;
        let r = b.enrich_concepts(&ks).expect("default stub is infallible");
        call_and_assert_not_implemented(r, "enrich_concepts");
    }

    #[test]
    fn enrich_metrics_remains_as_deferred_phase_a_stub() {
        let ks = Keyspace::new("test");
        let mut b = TestBackend;
        let r = b.enrich_metrics(&ks).expect("default stub is infallible");
        call_and_assert_not_implemented(r, "enrich_metrics");
    }
}

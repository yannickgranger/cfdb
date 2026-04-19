//! Enrichment port and report — split from [`crate::store::StoreBackend`] per
//! RFC-031 §2.
//!
//! The four `enrich_*` verbs live on [`EnrichBackend`], a sibling of
//! `StoreBackend`. Both are implemented by the same concrete backend
//! ([`cfdb_petgraph::PetgraphStore`] in v0.1), but consumers that only query
//! the graph depend on `StoreBackend` alone and do not pick up the enrichment
//! surface.
//!
//! In v0.1 (Phase A) every implementor inherits the default stubs returning
//! [`EnrichReport::not_implemented`]. Real implementations ship in v0.2 /
//! Phase D per RFC-032 §4 (Group D, issues #43–#48). Backends override each
//! method as the enrichment passes land.

use serde::{Deserialize, Serialize};

use crate::schema::Keyspace;
use crate::store::StoreError;

/// Summary of an enrichment pass over a keyspace.
///
/// Returned by every `enrich_*` verb on [`EnrichBackend`]. The shape is stable
/// across v0.1 → v0.2: Phase A implementations return a "not implemented"
/// report; Phase D implementations populate the counters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnrichReport {
    /// Verb name, e.g. `"enrich_docs"`.
    pub verb: String,
    /// Whether the pass actually ran. Phase A stubs return `false`.
    pub ran: bool,
    /// Facts (nodes + edges) the pass scanned. Zero for stubs.
    pub facts_scanned: u64,
    /// Attributes added or updated by the pass. Zero for stubs.
    pub attrs_written: u64,
    /// Edges added by the pass. Zero for stubs.
    pub edges_written: u64,
    /// Non-fatal warnings emitted during the pass.
    pub warnings: Vec<String>,
}

impl EnrichReport {
    /// Construct the canonical Phase A "not implemented" report for a verb.
    ///
    /// Used by Phase A implementations of `enrich_docs`, `enrich_metrics`,
    /// `enrich_history`, and `enrich_concepts`. The single warning makes the
    /// stub status visible in any caller that checks the warnings field
    /// (CLI dump, JSON RPC envelope, test assertions).
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

    /// True if the pass actually executed and produced data.
    pub fn is_complete(&self) -> bool {
        self.ran
    }
}

/// The enrichment port — sibling of [`crate::store::StoreBackend`].
///
/// Split from `StoreBackend` per RFC-031 §2 (ISP). v0.1 ships four default
/// stubs that return [`EnrichReport::not_implemented`]; concrete backends
/// override each method as Phase D enrichment passes land (RFC-032 §4 /
/// Group D, issues #43–#48).
///
/// The `Send + Sync` bounds match `StoreBackend` so the same concrete backend
/// can be handed to both traits without re-wrapping.
pub trait EnrichBackend: Send + Sync {
    /// Enrich a keyspace with documentation facts (rustdoc, README, RFC text).
    ///
    /// **Phase A stub:** the default implementation returns
    /// [`EnrichReport::not_implemented`]. Real implementations ship in v0.2 /
    /// Phase D per RFC-032 §4.
    fn enrich_docs(&mut self, _keyspace: &Keyspace) -> Result<EnrichReport, StoreError> {
        Ok(EnrichReport::not_implemented("enrich_docs"))
    }

    /// Enrich a keyspace with quality-signal facts (complexity, unwrap counts,
    /// clone-in-loop counts, etc.).
    ///
    /// **Phase A stub:** see [`Self::enrich_docs`].
    fn enrich_metrics(&mut self, _keyspace: &Keyspace) -> Result<EnrichReport, StoreError> {
        Ok(EnrichReport::not_implemented("enrich_metrics"))
    }

    /// Enrich a keyspace with git-history facts (last-touched, churn, author).
    ///
    /// **Phase A stub:** see [`Self::enrich_docs`].
    fn enrich_history(&mut self, _keyspace: &Keyspace) -> Result<EnrichReport, StoreError> {
        Ok(EnrichReport::not_implemented("enrich_history"))
    }

    /// Enrich a keyspace with bounded-context / concept facts (which crate
    /// owns which type, which concepts are canonical bypasses).
    ///
    /// **Phase A stub:** see [`Self::enrich_docs`].
    fn enrich_concepts(&mut self, _keyspace: &Keyspace) -> Result<EnrichReport, StoreError> {
        Ok(EnrichReport::not_implemented("enrich_concepts"))
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
}

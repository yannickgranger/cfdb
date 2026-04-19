//! Enrichment verbs and report — Phase A stub for #3628.
//!
//! The four `enrich_*` verbs in [`crate::store::StoreBackend`] return an
//! [`EnrichReport`] that summarises what an enrichment pass touched: how many
//! facts were read, how many edges/attributes were materialised, and any
//! warnings emitted along the way. In v0.1 (Phase A) every implementor returns
//! `EnrichReport::not_implemented(<verb>)`. The full enrichment passes ship in
//! v0.2 / Phase D per EPIC #3622.
//!
//! Keeping the verbs and report shape in cfdb-core now means consumers can
//! write code against the final API while the heavy lifting (git-history pass,
//! bounded-context heuristic, deprecation crawl, RFC-doc index, reachability
//! flood) is implemented incrementally in cfdb-petgraph and the enrichment
//! crates.

use serde::{Deserialize, Serialize};

/// Summary of an enrichment pass over a keyspace.
///
/// Returned by every `enrich_*` verb on [`crate::store::StoreBackend`]. The
/// shape is stable across v0.1 → v0.2: Phase A implementations return a
/// "not implemented" report; Phase D implementations populate the counters.
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

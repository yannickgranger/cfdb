//! Enrichment port and report — split from [`crate::store::StoreBackend`] per
//! RFC-031 §2.
//!
//! The `enrich_*` verbs live on [`EnrichBackend`], a sibling of
//! `StoreBackend`. Both are implemented by the same concrete backend
//! ([`cfdb_petgraph::PetgraphStore`] in v0.1), but consumers that only query
//! the graph depend on `StoreBackend` alone and do not pick up the enrichment
//! surface.
//!
//! # Pass vocabulary (post-#43 council round 1 — RFC addendum §A2.2)
//!
//! The trait surface defines **7 methods**:
//!
//! | Method | Pass | Scope |
//! |---|---|---|
//! | [`enrich_git_history`][EnrichBackend::enrich_git_history] | §A2.2 row 1 | commit age, author, churn per `:Item` |
//! | [`enrich_rfc_docs`][EnrichBackend::enrich_rfc_docs] | §A2.2 row 2 | `(:Item)-[:REFERENCED_BY]->(:RfcDoc)` edges |
//! | [`enrich_deprecation`][EnrichBackend::enrich_deprecation] | §A2.2 row 3 | `:Item.is_deprecated` + `deprecation_since` (extractor-time per RFC amendment) |
//! | [`enrich_bounded_context`][EnrichBackend::enrich_bounded_context] | §A2.2 row 4 | re-enrichment of `:Item.bounded_context` after TOML changes |
//! | [`enrich_concepts`][EnrichBackend::enrich_concepts] | §A2.2 row 6 | `:Concept` node materialization from `.cfdb/concepts/*.toml` |
//! | [`enrich_reachability`][EnrichBackend::enrich_reachability] | §A2.2 row 5 | BFS from `:EntryPoint` over `CALLS*` |
//! | [`enrich_metrics`][EnrichBackend::enrich_metrics] | deferred | orthogonal quality signals; Phase A stub retained out of #43 scope |
//!
//! In v0.1 (Phase A) every implementor inherits the default stubs returning
//! [`EnrichReport::not_implemented`]. Real implementations ship in v0.2 /
//! Phase D per RFC-032 §4 and #43 slices 43-B through 43-G. Backends override
//! each method as the enrichment passes land.

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
/// Split from `StoreBackend` per RFC-031 §2 (ISP). v0.1 shipped four default
/// stubs; #43-A expands the surface to seven default stubs matching the
/// post-amendment RFC addendum §A2.2 pass table. Each method returns
/// [`EnrichReport::not_implemented`] until its sibling slice (43-B through
/// 43-G) lands a real implementation on the concrete backend.
///
/// The `Send + Sync` bounds match `StoreBackend` so the same concrete backend
/// can be handed to both traits without re-wrapping.
pub trait EnrichBackend: Send + Sync {
    /// Enrich a keyspace with git-history facts — commit age, author, churn
    /// count per `:Item` file. Writes `git_last_commit_unix_ts` (i64 epoch —
    /// NOT days, per clean-arch B2 determinism), `git_last_author`,
    /// `git_commit_count`.
    ///
    /// **Phase A stub:** the default implementation returns
    /// [`EnrichReport::not_implemented`]. Implementation lands in slice 43-B
    /// (issue #105) behind the `git-enrich` feature flag.
    fn enrich_git_history(&mut self, _keyspace: &Keyspace) -> Result<EnrichReport, StoreError> {
        Ok(EnrichReport::not_implemented("enrich_git_history"))
    }

    /// Enrich a keyspace with RFC-reference facts — scan `.concept-graph/*.md`
    /// and `docs/rfc/*.md` for concept-name matches, emit `:RfcDoc` nodes and
    /// `(:Item)-[:REFERENCED_BY]->(:RfcDoc)` edges.
    ///
    /// **Phase A stub:** see [`Self::enrich_git_history`]. Implementation
    /// lands in slice 43-D (issue #107).
    ///
    /// **Scope narrowing (RFC amendment §A2.2):** this pass covers RFC-file
    /// keyword matching only. Broader rustdoc rendering was implied by the
    /// former `enrich_docs` stub comment but is a non-goal for v0.2; deferred
    /// beyond the #43 slice set.
    fn enrich_rfc_docs(&mut self, _keyspace: &Keyspace) -> Result<EnrichReport, StoreError> {
        Ok(EnrichReport::not_implemented("enrich_rfc_docs"))
    }

    /// Enrich a keyspace with deprecation facts — `:Item.is_deprecated` (bool),
    /// `:Item.deprecation_since` (optional version string from
    /// `#[deprecated(since = "X")]`).
    ///
    /// **Phase A stub:** see [`Self::enrich_git_history`]. The real work
    /// lands in slice 43-C (issue #106) as an **extractor extension** in
    /// `cfdb-extractor/src/attrs.rs`, not as a Phase D enrichment — the
    /// `#[deprecated]` attribute is a syntactic fact and the extractor
    /// already walks attributes. The `PetgraphStore::enrich_deprecation`
    /// override will return a `ran: true, attrs_written: 0` report naming
    /// the extractor as the real source.
    fn enrich_deprecation(&mut self, _keyspace: &Keyspace) -> Result<EnrichReport, StoreError> {
        Ok(EnrichReport::not_implemented("enrich_deprecation"))
    }

    /// Re-enrich a keyspace's `:Item.bounded_context` attribute after
    /// `.cfdb/concepts/*.toml` has changed. **Mostly a no-op on fresh
    /// extractions** — `cfdb-extractor` already populates
    /// `:Item.bounded_context` at extraction time. This pass patches
    /// mismatches when TOML declarations changed without a full re-extract.
    ///
    /// **Phase A stub:** see [`Self::enrich_git_history`]. Implementation
    /// lands in slice 43-E (issue #108). Slice 43-E carries the v0.2-9
    /// ≥95% accuracy gate which BLOCKS merge of both slice 43-E and the
    /// downstream classifier (#48) per synthesis invariant I6.
    fn enrich_bounded_context(&mut self, _keyspace: &Keyspace) -> Result<EnrichReport, StoreError> {
        Ok(EnrichReport::not_implemented("enrich_bounded_context"))
    }

    /// Materialize `:Concept` nodes from `.cfdb/concepts/<name>.toml`
    /// declarations and emit `(:Item)-[:LABELED_AS]->(:Concept)` +
    /// `(:Item)-[:CANONICAL_FOR]->(:Concept)` edges for items in each
    /// context's declared `canonical_crate`.
    ///
    /// **Phase A stub:** see [`Self::enrich_git_history`]. Implementation
    /// lands in slice 43-F (issue #109) and unblocks issues #101 (Trigger
    /// T1) and #102 (Trigger T3) which consume `:Concept` nodes.
    ///
    /// **Scope note (DDD Q4):** this method name existed in v0.1 Phase A
    /// with a conflated scope ("bounded-context / concept facts"). Post
    /// #43-A amendment, the scope narrows to `:Concept` node materialization
    /// only. Context-assignment work was never this pass's responsibility
    /// — `cfdb-extractor` owns it at extraction time; slice 43-E handles
    /// re-enrichment.
    fn enrich_concepts(&mut self, _keyspace: &Keyspace) -> Result<EnrichReport, StoreError> {
        Ok(EnrichReport::not_implemented("enrich_concepts"))
    }

    /// Enrich a keyspace with entry-point-reachability facts — BFS from every
    /// `:EntryPoint` over `CALLS*` edges, writing `:Item.reachable_from_entry`
    /// (bool) and `:Item.reachable_entry_count` (i64).
    ///
    /// **Phase A stub:** see [`Self::enrich_git_history`]. Implementation
    /// lands in slice 43-G (issue #110) and consumes `:EntryPoint` nodes
    /// produced by `cfdb-hir-extractor` (v0.2+). When the keyspace has zero
    /// `:EntryPoint` nodes, the real implementation returns `ran: false` +
    /// a clear warning rather than silently marking all items unreachable
    /// (clean-arch B3 degraded path).
    fn enrich_reachability(&mut self, _keyspace: &Keyspace) -> Result<EnrichReport, StoreError> {
        Ok(EnrichReport::not_implemented("enrich_reachability"))
    }

    /// Enrich a keyspace with quality-signal facts (complexity, unwrap counts,
    /// clone-in-loop counts, etc.).
    ///
    /// **Deferred — out of #43 scope per RFC amendment §A2.2.** The quality
    /// metrics concern is orthogonal to the debt-cause classifier pipeline:
    /// the six classes in §A2.1 do not consume these signals, so no #43
    /// slice implements this pass. Retained as a Phase A stub so the surface
    /// is stable; a future RFC may resuscitate it behind its own issue.
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

    // ---- #43-A: round-trip tests for renamed + newly added stubs --------
    //
    // The trait surface expands from 4 methods to 7 per RFC addendum §A2.2
    // (post #43 council round 1 amendment): rename `enrich_docs` →
    // `enrich_rfc_docs`, `enrich_history` → `enrich_git_history`; add
    // `enrich_bounded_context`, `enrich_deprecation`, `enrich_reachability`;
    // keep `enrich_concepts` (scope narrowed to `:Concept` materialization)
    // and `enrich_metrics` (deferred Phase A stub). These tests exercise
    // every method's default stub via an empty backend struct, proving the
    // port surface exists and the canonical `not_implemented` shape is
    // returned.

    /// A minimal test backend that takes every default impl from
    /// `EnrichBackend`. The `Send + Sync` bounds are satisfied trivially
    /// (zero state).
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
        // `enrich_metrics` is explicitly deferred per RFC addendum §A2.2
        // (council round 1 amendment §43-A): orthogonal to the debt-cause
        // classifier pipeline. Retained as a Phase A stub; this test pins
        // the contract so a future refactor can't accidentally drop it.
        let ks = Keyspace::new("test");
        let mut b = TestBackend;
        let r = b.enrich_metrics(&ks).expect("default stub is infallible");
        call_and_assert_not_implemented(r, "enrich_metrics");
    }
}

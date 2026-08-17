//! `EnrichBackend` implementation for `PetgraphStore`.
//!
//! Enrichment is a sibling trait. PetgraphStore inherits the
//! seven stubs (`EnrichReport::not_implemented`); concrete enrichment
//! passes override individual methods.
//!
//! `enrich_deprecation` overridden to report the real
//! source as the extractor rather than deflecting to `not_implemented`. The
//! deprecation facts (`is_deprecated`, `deprecation_since`) are populated at
//! extraction time by `cfdb-extractor` via `extract_deprecated_attr`, so the
//! `EnrichBackend::enrich_deprecation` method is a runtime no-op but must
//! advertise its non-stub status — `ran: true, attrs_written: 0` with a
//! warning naming the extractor so callers can distinguish "done upstream"
//! from "deferred".

#[cfg(any(feature = "git-enrich", feature = "quality-metrics"))]
use std::path::PathBuf;

use cfdb_core::enrich::EnrichBackend;
use cfdb_core::store::StoreError;

use crate::PetgraphStore;

impl PetgraphStore {
    /// Guard #1 — keyspace existence. Returns `Err(UnknownKeyspace)` if the
    /// caller's target keyspace is not known to the store; otherwise `Ok(())`.
    fn require_keyspace(&self, keyspace: &cfdb_core::schema::Keyspace) -> Result<(), StoreError> {
        if !self.keyspaces.contains_key(keyspace) {
            return Err(StoreError::UnknownKeyspace(keyspace.clone()));
        }
        Ok(())
    }

    /// Guard #2 — `workspace_root` presence. Returns `Ok(root)` if the
    /// store has a workspace_root attached, otherwise `Err(degraded report)`
    /// so the caller can early-return the degraded report unchanged.
    /// `purpose_suffix` is the per-verb explanation of what the pass would
    /// do with the workspace root (e.g. "scan docs/ for RFC references") —
    /// these are user-facing diagnostics that vary meaningfully per verb.
    ///
    /// Feature-gated: `enrich_rfc_docs`/`enrich_bounded_context`/
    /// `enrich_concepts` — the only unconditional callers — moved to
    /// `cfdb-enrich::EnrichEngine` (RFC-056 056-A/B/C), leaving only the
    /// feature-gated `enrich_git_history`/`enrich_metrics` dispatch arms;
    /// a default (no-features) build would otherwise flag this dead.
    #[cfg(any(feature = "git-enrich", feature = "quality-metrics"))]
    fn require_workspace(
        &self,
        verb: &'static str,
        purpose_suffix: &str,
    ) -> Result<PathBuf, cfdb_core::enrich::EnrichReport> {
        if let Some(root) = self.workspace_root.clone() {
            return Ok(root);
        }
        Err(cfdb_core::enrich::EnrichReport {
            verb: verb.into(),
            ran: false,
            facts_scanned: 0,
            attrs_written: 0,
            edges_written: 0,
            warnings: vec![format!(
                "{verb}: no workspace_root attached to PetgraphStore — construct via `PetgraphStore::new().with_workspace(root)` {purpose_suffix}"
            )],
        })
    }
}

impl EnrichBackend for PetgraphStore {
    fn enrich_deprecation(
        &mut self,
        keyspace: &cfdb_core::schema::Keyspace,
    ) -> Result<cfdb_core::enrich::EnrichReport, StoreError> {
        self.require_keyspace(keyspace)?;
        Ok(cfdb_core::enrich::EnrichReport {
            verb: "enrich_deprecation".into(),
            ran: true,
            facts_scanned: 0,
            attrs_written: 0,
            edges_written: 0,
            warnings: vec![
                "enrich_deprecation: facts populated at extraction time by cfdb-extractor::extract_deprecated_attr (#43-C / RFC addendum §A2.2 row 3); no enrichment work to do"
                    .into(),
            ],
        })
    }

    fn enrich_git_history(
        &mut self,
        keyspace: &cfdb_core::schema::Keyspace,
    ) -> Result<cfdb_core::enrich::EnrichReport, StoreError> {
        self.require_keyspace(keyspace)?;
        Ok(enrich_git_history_dispatch(self, keyspace))
    }

    // enrich_rfc_docs moved to cfdb-enrich::EnrichEngine (RFC-056 056-A) —
    // falls through to EnrichBackend's default not_implemented stub on
    // PetgraphStore now; cfdb-cli's dispatcher no longer calls this arm.

    // enrich_bounded_context moved to cfdb-enrich::EnrichEngine (RFC-056
    // 056-B) — falls through to EnrichBackend's default not_implemented
    // stub on PetgraphStore now; cfdb-cli's dispatcher no longer calls this
    // arm.

    // enrich_concepts moved to cfdb-enrich::EnrichEngine (RFC-056 056-C) —
    // falls through to EnrichBackend's default not_implemented stub on
    // PetgraphStore now; cfdb-cli's dispatcher no longer calls this arm.

    fn enrich_reachability(
        &mut self,
        keyspace: &cfdb_core::schema::Keyspace,
    ) -> Result<cfdb_core::enrich::EnrichReport, StoreError> {
        self.require_keyspace(keyspace)?;
        // Reachability is purely graph-internal — no filesystem access, so
        // no `workspace_root` check (unlike the TOML/git/rfc-scanning passes).
        let state = self
            .keyspaces
            .get_mut(keyspace)
            .expect("keyspace presence checked above");
        // Two passes — first the All-kinds BFS that writes
        // `reachable_from_entry`, then the ProductionOnly BFS that excludes
        // `kind ∈ {test, bench}` and writes `reachable_from_production_entry`.
        // The trait surface stays single-call; the dual-pass orchestration
        // is encapsulated here.
        use crate::enrich::reachability::ReachabilityFilter;
        let pass_all = crate::enrich::reachability::run(state, ReachabilityFilter::All);
        if !pass_all.ran {
            // Degraded path (zero entry points) — Pass 2 would degrade the
            // same way; surface the single warning and skip.
            return Ok(pass_all);
        }
        let pass_prod = crate::enrich::reachability::run(state, ReachabilityFilter::ProductionOnly);
        let mut warnings = pass_all.warnings;
        warnings.extend(pass_prod.warnings);
        Ok(cfdb_core::enrich::EnrichReport {
            verb: pass_all.verb,
            ran: pass_all.ran && pass_prod.ran,
            facts_scanned: pass_all.facts_scanned,
            attrs_written: pass_all.attrs_written + pass_prod.attrs_written,
            edges_written: 0,
            warnings,
        })
    }

    fn enrich_metrics(
        &mut self,
        keyspace: &cfdb_core::schema::Keyspace,
    ) -> Result<cfdb_core::enrich::EnrichReport, StoreError> {
        self.require_keyspace(keyspace)?;
        Ok(enrich_metrics_dispatch(self, keyspace))
    }
}

/// Feature-off path — `quality-metrics` gates syn (+ sha2) out of default
/// builds. Without the feature the verb still exists and dispatches here,
/// returning a `ran: false` report whose warning names the feature flag.
#[cfg(not(feature = "quality-metrics"))]
fn enrich_metrics_dispatch(
    _store: &mut PetgraphStore,
    _keyspace: &cfdb_core::schema::Keyspace,
) -> cfdb_core::enrich::EnrichReport {
    cfdb_core::enrich::EnrichReport {
        verb: "enrich_metrics".into(),
        ran: false,
        facts_scanned: 0,
        attrs_written: 0,
        edges_written: 0,
        warnings: vec![
            "enrich_metrics: built without `quality-metrics` feature — recompile `cfdb-cli` with `--features quality-metrics` to populate unwrap_count + cyclomatic + dup_cluster_id (and additionally `--features llvm-cov` for test_coverage) per RFC-036 §3.3 / issue #203"
                .into(),
        ],
    }
}

/// Feature-on path — requires a `workspace_root` on the store so syn can
/// re-parse source files referenced by `:Item{kind:"Fn"}.file`. If the
/// store was built without one, return a `ran: false` degraded report
/// naming the configuration gap.
#[cfg(feature = "quality-metrics")]
fn enrich_metrics_dispatch(
    store: &mut PetgraphStore,
    keyspace: &cfdb_core::schema::Keyspace,
) -> cfdb_core::enrich::EnrichReport {
    let root = match store.require_workspace(
        "enrich_metrics",
        "so the pass can re-parse source files referenced by :Item{kind:Fn}.file",
    ) {
        Ok(r) => r,
        Err(report) => return report,
    };
    let state = store
        .keyspaces
        .get_mut(keyspace)
        .expect("keyspace presence checked by caller");
    crate::enrich::metrics::run(state, &root, &crate::enrich::metrics::Config::default())
}

/// Feature-off path — the real pass is gated on `git-enrich` to keep
/// libgit2 out of default builds. Without the feature the verb still
/// exists and dispatches here, returning a `ran: false` report whose
/// warning names the feature flag.
#[cfg(not(feature = "git-enrich"))]
fn enrich_git_history_dispatch(
    _store: &mut PetgraphStore,
    _keyspace: &cfdb_core::schema::Keyspace,
) -> cfdb_core::enrich::EnrichReport {
    cfdb_core::enrich::EnrichReport {
        verb: "enrich_git_history".into(),
        ran: false,
        facts_scanned: 0,
        attrs_written: 0,
        edges_written: 0,
        warnings: vec![
            "enrich_git_history: built without `git-enrich` feature — recompile `cfdb-cli` with `--features git-enrich` to populate git-history facts (RFC addendum §A2.2 row 1 / issue #105)"
                .into(),
        ],
    }
}

/// Feature-on path — requires a `workspace_root` on the store. If the store
/// was built without one (most test sites and tool-free callers), return a
/// `ran: false` degraded report so the caller sees the configuration gap
/// rather than silent Nulls.
#[cfg(feature = "git-enrich")]
fn enrich_git_history_dispatch(
    store: &mut PetgraphStore,
    keyspace: &cfdb_core::schema::Keyspace,
) -> cfdb_core::enrich::EnrichReport {
    let root = match store.require_workspace(
        "enrich_git_history",
        "so the pass can open a git repository",
    ) {
        Ok(r) => r,
        Err(report) => return report,
    };
    let state = store
        .keyspaces
        .get_mut(keyspace)
        .expect("keyspace presence checked by caller");
    crate::enrich::git_history::run(state, &root)
}

// ---------------------------------------------------------------------------
// Characterization tests — pre-strangler-fig safety net.
//
// These pin what this dispatcher does *today*, verbatim (exact report
// fields, exact warning text), so a later extraction of the enrichment
// passes into their own crate (audit 2026-08-17: cfdb-petgraph bundles
// storage + eval + enrichment with zero shared commit history between
// enrich/ and eval/) can be checked byte-for-byte against this baseline.
// A test failing here after a refactor means behavior changed, not that
// the old behavior was correct — whether it *should* change is a separate,
// later question these tests deliberately do not answer.
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use cfdb_core::enrich::EnrichBackend;
    use cfdb_core::schema::Keyspace;
    use cfdb_core::store::StoreBackend;

    use crate::PetgraphStore;

    fn store_with_empty_keyspace(ks: &Keyspace) -> PetgraphStore {
        let mut store = PetgraphStore::new();
        store.ingest_nodes(ks, vec![]).expect("register keyspace");
        store
    }

    #[test]
    fn deprecation_pins_fixed_report_shape() {
        let ks = Keyspace::new("test");
        let mut store = store_with_empty_keyspace(&ks);

        let report = store.enrich_deprecation(&ks).expect("pass");

        assert!(report.ran);
        assert_eq!(report.facts_scanned, 0);
        assert_eq!(report.attrs_written, 0);
        assert_eq!(report.edges_written, 0);
        assert_eq!(
            report.warnings,
            vec!["enrich_deprecation: facts populated at extraction time by \
                 cfdb-extractor::extract_deprecated_attr (#43-C / RFC \
                 addendum §A2.2 row 3); no enrichment work to do"
                .to_string()]
        );
    }

    #[test]
    fn deprecation_unknown_keyspace_returns_err() {
        let mut store = PetgraphStore::new();
        let ks = Keyspace::new("never");

        let err = store
            .enrich_deprecation(&ks)
            .expect_err("unknown keyspace must err");

        assert!(format!("{err:?}").contains("UnknownKeyspace"));
    }

    // `quality-metrics` is off in the default build (`default = []` in
    // Cargo.toml) — CI's `--all-features` run never exercises this branch,
    // so this test only compiles/runs under a plain `cargo test -p
    // cfdb-petgraph` (no extra flags).
    #[cfg(not(feature = "quality-metrics"))]
    #[test]
    fn enrich_metrics_feature_off_pins_degraded_report() {
        let ks = Keyspace::new("test");
        let mut store = store_with_empty_keyspace(&ks);

        let report = store.enrich_metrics(&ks).expect("pass");

        assert!(!report.ran);
        assert_eq!(report.facts_scanned, 0);
        assert_eq!(report.attrs_written, 0);
        assert_eq!(report.edges_written, 0);
        assert_eq!(
            report.warnings,
            vec!["enrich_metrics: built without `quality-metrics` feature — \
                 recompile `cfdb-cli` with `--features quality-metrics` to \
                 populate unwrap_count + cyclomatic + dup_cluster_id (and \
                 additionally `--features llvm-cov` for test_coverage) per \
                 RFC-036 §3.3 / issue #203"
                .to_string()]
        );
    }

    // Same rationale as above — `git-enrich` is off in the default build.
    #[cfg(not(feature = "git-enrich"))]
    #[test]
    fn enrich_git_history_feature_off_pins_degraded_report() {
        let ks = Keyspace::new("test");
        let mut store = store_with_empty_keyspace(&ks);

        let report = store.enrich_git_history(&ks).expect("pass");

        assert!(!report.ran);
        assert_eq!(report.facts_scanned, 0);
        assert_eq!(report.attrs_written, 0);
        assert_eq!(report.edges_written, 0);
        assert_eq!(
            report.warnings,
            vec!["enrich_git_history: built without `git-enrich` feature — \
                 recompile `cfdb-cli` with `--features git-enrich` to \
                 populate git-history facts (RFC addendum §A2.2 row 1 / \
                 issue #105)"
                .to_string()]
        );
    }
}

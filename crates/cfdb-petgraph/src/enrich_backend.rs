//! `EnrichBackend` implementation for `PetgraphStore`.
//!
//! RFC-031 §2 — enrichment is a sibling trait. PetgraphStore inherits the
//! seven Phase A stubs (`EnrichReport::not_implemented`); concrete enrichment
//! passes override individual methods as #43 slices land.
//!
//! `enrich_deprecation` overridden in slice 43-C (#106) to report the real
//! source as the extractor rather than deflecting to `not_implemented`. The
//! deprecation facts (`is_deprecated`, `deprecation_since`) are populated at
//! extraction time by `cfdb-extractor` via `extract_deprecated_attr`, so the
//! `EnrichBackend::enrich_deprecation` method is a runtime no-op but must
//! advertise its non-stub status — `ran: true, attrs_written: 0` with a
//! warning naming the extractor so callers can distinguish "done upstream"
//! from "deferred".

use std::path::PathBuf;

use cfdb_core::enrich::EnrichBackend;
use cfdb_core::store::StoreError;

use crate::PetgraphStore;

impl PetgraphStore {
    /// Guard #1 — keyspace existence. Returns `Err(UnknownKeyspace)` if the
    /// caller's target keyspace is not known to the store; otherwise `Ok(())`.
    /// Audit 2026-W17 / EPIC #273 / Pattern 3 (cfdb-petgraph F-002): this
    /// guard was duplicated across 7 enrich verbs.
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
    /// do with the workspace root (e.g. "scan docs/ for RFC references")
    /// and is preserved verbatim from the previous inline strings — these
    /// are user-facing diagnostics that vary meaningfully per verb.
    /// Audit 2026-W17 / EPIC #273 / Pattern 3 (cfdb-petgraph F-002).
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
        // Keyspace existence check mirrors other enrichment verbs — a
        // caller targeting an unknown keyspace gets the same error shape
        // as `schema_version`/`drop_keyspace`.
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

    fn enrich_rfc_docs(
        &mut self,
        keyspace: &cfdb_core::schema::Keyspace,
    ) -> Result<cfdb_core::enrich::EnrichReport, StoreError> {
        self.require_keyspace(keyspace)?;
        let root = match self.require_workspace(
            "enrich_rfc_docs",
            "so the pass can scan docs/ for RFC references",
        ) {
            Ok(r) => r,
            Err(report) => return Ok(report),
        };
        let state = self
            .keyspaces
            .get_mut(keyspace)
            .expect("keyspace presence checked above");
        Ok(crate::enrich::rfc_docs::run(state, &root))
    }

    fn enrich_bounded_context(
        &mut self,
        keyspace: &cfdb_core::schema::Keyspace,
    ) -> Result<cfdb_core::enrich::EnrichReport, StoreError> {
        self.require_keyspace(keyspace)?;
        let root = match self.require_workspace(
            "enrich_bounded_context",
            "so the pass can read `.cfdb/concepts/*.toml`",
        ) {
            Ok(r) => r,
            Err(report) => return Ok(report),
        };
        let state = self
            .keyspaces
            .get_mut(keyspace)
            .expect("keyspace presence checked above");
        Ok(crate::enrich::bounded_context::run(state, &root))
    }

    fn enrich_concepts(
        &mut self,
        keyspace: &cfdb_core::schema::Keyspace,
    ) -> Result<cfdb_core::enrich::EnrichReport, StoreError> {
        self.require_keyspace(keyspace)?;
        let root = match self.require_workspace(
            "enrich_concepts",
            "so the pass can read `.cfdb/concepts/*.toml`",
        ) {
            Ok(r) => r,
            Err(report) => return Ok(report),
        };
        let state = self
            .keyspaces
            .get_mut(keyspace)
            .expect("keyspace presence checked above");
        Ok(crate::enrich::concepts::run(state, &root))
    }

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
        Ok(crate::enrich::reachability::run(state))
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
/// returning a `ran: false` report whose warning names the feature flag
/// (RFC-036 §3.3 / issue #203).
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
/// naming the configuration gap (mirrors `enrich_git_history_dispatch`).
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

/// Feature-off path — the real pass is gated on `git-enrich` to keep libgit2
/// out of default builds (rust-systems Q1 / Q6). Without the feature the verb
/// still exists and dispatches here, returning a `ran: false` report whose
/// warning names the feature flag (AC-1 / issue #105).
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

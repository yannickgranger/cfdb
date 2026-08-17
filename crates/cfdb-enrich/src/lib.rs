//! `EnrichEngine` implements [`cfdb_core::enrich::EnrichBackend`]
//! generically over any [`cfdb_core::graph::GraphBackend`] implementor, so
//! this crate never depends on `cfdb-petgraph` (or any concrete storage
//! engine) in production — only in `[dev-dependencies]`, for the pass test
//! suites, which need a concrete backend to run against.
//!
//! Pure dispatch + the two guards every `EnrichBackend` verb needs
//! (keyspace existence, workspace-root presence) — no pass-level logic
//! lives here. A verb with no real implementation falls through to
//! `EnrichBackend`'s default `not_implemented` stub.

use std::path::PathBuf;

use cfdb_core::enrich::{EnrichBackend, EnrichReport};
use cfdb_core::graph::GraphBackend;
use cfdb_core::schema::Keyspace;
use cfdb_core::store::StoreError;

mod bounded_context;
mod concepts;
#[cfg(feature = "git-enrich")]
mod git_history;
#[cfg(feature = "quality-metrics")]
mod metrics;
mod rfc_docs;

/// Wraps any [`GraphBackend`] implementor and dispatches the 7 `enrich_*`
/// verbs against it. Borrows the store rather than owning it — mirrors
/// `PetgraphStore`'s enrich dispatch, which never took ownership either.
pub struct EnrichEngine<'s, S> {
    store: &'s mut S,
}

impl<'s, S: GraphBackend> EnrichEngine<'s, S> {
    pub fn new(store: &'s mut S) -> Self {
        Self { store }
    }

    /// Guard #2 — `workspace_root` presence. Returns `Ok(root)` if the
    /// wrapped store has a workspace_root attached, otherwise
    /// `Err(degraded report)` so the caller can early-return the degraded
    /// report unchanged.
    ///
    /// The message names `PetgraphStore` explicitly, which is only
    /// accurate while it's the sole `GraphBackend` implementor.
    fn require_workspace(
        &self,
        verb: &'static str,
        purpose_suffix: &str,
    ) -> Result<PathBuf, EnrichReport> {
        if let Some(root) = self.store.workspace_root() {
            return Ok(root.to_path_buf());
        }
        Err(EnrichReport {
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

impl<'s, S: GraphBackend> EnrichBackend for EnrichEngine<'s, S> {
    fn enrich_deprecation(&mut self, keyspace: &Keyspace) -> Result<EnrichReport, StoreError> {
        // Deprecation facts are populated at extraction time, not by this
        // pass — but an unknown keyspace must still error.
        let _ = self.store.graph_view(keyspace)?;
        Ok(EnrichReport {
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

    fn enrich_rfc_docs(&mut self, keyspace: &Keyspace) -> Result<EnrichReport, StoreError> {
        // Guard order is load-bearing: an unknown keyspace must error even
        // when workspace_root is also missing, not degrade silently.
        let _ = self.store.graph_view(keyspace)?;
        let root = match self.require_workspace(
            "enrich_rfc_docs",
            "so the pass can scan docs/ for RFC references",
        ) {
            Ok(root) => root,
            Err(report) => return Ok(report),
        };
        let view = self.store.graph_view(keyspace)?;
        Ok(rfc_docs::run(view, &root))
    }

    fn enrich_bounded_context(&mut self, keyspace: &Keyspace) -> Result<EnrichReport, StoreError> {
        // See enrich_rfc_docs — same guard-order invariant.
        let _ = self.store.graph_view(keyspace)?;
        let root = match self.require_workspace(
            "enrich_bounded_context",
            "so the pass can read `.cfdb/concepts/*.toml`",
        ) {
            Ok(root) => root,
            Err(report) => return Ok(report),
        };
        let view = self.store.graph_view(keyspace)?;
        Ok(bounded_context::run(view, &root))
    }

    fn enrich_concepts(&mut self, keyspace: &Keyspace) -> Result<EnrichReport, StoreError> {
        // See enrich_rfc_docs — same guard-order invariant.
        let _ = self.store.graph_view(keyspace)?;
        let root = match self.require_workspace(
            "enrich_concepts",
            "so the pass can read `.cfdb/concepts/*.toml`",
        ) {
            Ok(root) => root,
            Err(report) => return Ok(report),
        };
        let view = self.store.graph_view(keyspace)?;
        Ok(concepts::run(view, &root))
    }

    #[cfg(feature = "git-enrich")]
    fn enrich_git_history(&mut self, keyspace: &Keyspace) -> Result<EnrichReport, StoreError> {
        let _ = self.store.graph_view(keyspace)?;
        let root = match self.require_workspace(
            "enrich_git_history",
            "so the pass can open a git repository",
        ) {
            Ok(root) => root,
            Err(report) => return Ok(report),
        };
        let view = self.store.graph_view(keyspace)?;
        Ok(git_history::run(view, &root))
    }

    /// No workspace check — feature absence alone determines the report.
    #[cfg(not(feature = "git-enrich"))]
    fn enrich_git_history(&mut self, keyspace: &Keyspace) -> Result<EnrichReport, StoreError> {
        let _ = self.store.graph_view(keyspace)?;
        Ok(EnrichReport {
            verb: "enrich_git_history".into(),
            ran: false,
            facts_scanned: 0,
            attrs_written: 0,
            edges_written: 0,
            warnings: vec![
                "enrich_git_history: built without `git-enrich` feature — recompile `cfdb-cli` with `--features git-enrich` to populate git-history facts (RFC addendum §A2.2 row 1 / issue #105)"
                    .into(),
            ],
        })
    }

    /// Always the default `Config` — nothing surfaces a coverage-JSON path
    /// yet, so `test_coverage` never populates through this call site.
    #[cfg(feature = "quality-metrics")]
    fn enrich_metrics(&mut self, keyspace: &Keyspace) -> Result<EnrichReport, StoreError> {
        let _ = self.store.graph_view(keyspace)?;
        let root = match self.require_workspace(
            "enrich_metrics",
            "so the pass can re-parse source files referenced by :Item{kind:Fn}.file",
        ) {
            Ok(root) => root,
            Err(report) => return Ok(report),
        };
        let view = self.store.graph_view(keyspace)?;
        Ok(metrics::run(view, &root, &metrics::Config::default()))
    }

    /// No workspace check — feature absence alone determines the report.
    #[cfg(not(feature = "quality-metrics"))]
    fn enrich_metrics(&mut self, keyspace: &Keyspace) -> Result<EnrichReport, StoreError> {
        let _ = self.store.graph_view(keyspace)?;
        Ok(EnrichReport {
            verb: "enrich_metrics".into(),
            ran: false,
            facts_scanned: 0,
            attrs_written: 0,
            edges_written: 0,
            warnings: vec![
                "enrich_metrics: built without `quality-metrics` feature — recompile `cfdb-cli` with `--features quality-metrics` to populate unwrap_count + cyclomatic + dup_cluster_id (and additionally `--features llvm-cov` for test_coverage) per RFC-036 §3.3 / issue #203"
                    .into(),
            ],
        })
    }
}

#[cfg(test)]
mod tests;

//! `EnrichEngine` — the strangler-fig destination for `cfdb-petgraph`'s 7
//! enrichment passes (RFC-056).
//!
//! Implements [`cfdb_core::enrich::EnrichBackend`] generically over any
//! [`cfdb_core::graph::GraphBackend`] implementor, so this crate never
//! depends on `cfdb-petgraph` (or any concrete storage engine) in
//! production — only in `[dev-dependencies]`, for the moved passes' own
//! test suites.
//!
//! Pure dispatch + the two guards every `EnrichBackend` verb already needs
//! (keyspace existence, workspace-root presence) — no pass-level logic
//! lives here. As of slice 056-0, no pass has moved yet; every verb except
//! `enrich_deprecation` still falls through to `EnrichBackend`'s default
//! `not_implemented` stub. Slices 056-A through 056-F each add one real
//! dispatch arm as their pass moves in.

use std::path::PathBuf;

use cfdb_core::enrich::{EnrichBackend, EnrichReport};
use cfdb_core::graph::GraphBackend;
use cfdb_core::schema::Keyspace;
use cfdb_core::store::StoreError;

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
    /// report unchanged. Moved verbatim from
    /// `cfdb-petgraph::enrich_backend.rs`'s `require_workspace` (RFC-056
    /// §3.2) — same warning text, so a pass's characterization test stays
    /// byte-identical once it moves here.
    ///
    /// Byte-identity with the pre-move warning text is pinned by
    /// `require_workspace_degraded_report_pins_exact_warning_text`
    /// (`tests.rs`) as of 056-0 itself — no need to wait for 056-A to
    /// discover a mismatch. TODO(056-G): the message names `PetgraphStore`
    /// by name, which stops being generically true once a second
    /// `GraphBackend` implementor exists; revisit the wording when this
    /// crate's last caller-facing tie to the concrete backend is cut.
    ///
    /// No PRODUCTION caller yet as of slice 056-0 (only `enrich_deprecation`
    /// is dispatched here, and it needs no workspace root) — hence
    /// `#[allow(dead_code)]`: a plain `cargo build` doesn't see the
    /// `#[cfg(test)]` callers above. Wired up starting slice 056-A
    /// (`enrich_rfc_docs`, the first pass that reads `workspace_root`).
    #[allow(dead_code)]
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
        // No port access needed — deprecation facts are extractor-time, not
        // an enrichment pass. Still resolve the keyspace via graph_view
        // (discarding the view) so an unknown keyspace still errs, matching
        // PetgraphStore::enrich_deprecation's pre-existing behavior
        // (characterized in PR #575's deprecation_unknown_keyspace_returns_err).
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

    // The other 6 verbs (enrich_git_history, enrich_rfc_docs,
    // enrich_bounded_context, enrich_concepts, enrich_reachability,
    // enrich_metrics) are not overridden here — they fall through to
    // EnrichBackend's default `EnrichReport::not_implemented(...)` stub
    // until their slice (056-A through 056-F) moves the real pass in.
}

#[cfg(test)]
mod tests;

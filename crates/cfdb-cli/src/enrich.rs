//! `cfdb enrich-*` verbs.
//!
//! Public surface: every item here is re-exported from the crate root.
//! See [`cfdb_core::enrich::EnrichBackend`] for the trait-level surface
//! and the per-pass slice that ships the real implementation.

use std::path::PathBuf;

use cfdb_core::enrich::{EnrichBackend, EnrichReport};

use crate::compose;
use crate::output;

/// Which `enrich_*` verb to dispatch to. Lets one handler function service
/// all CLI variants without duplicating the load-store-print boilerplate.
pub enum EnrichVerb {
    /// `enrich_git_history` — commit age/author/churn per `:Item`.
    GitHistory,
    /// `enrich_rfc_docs` — `:RfcDoc` + `REFERENCED_BY` emission.
    RfcDocs,
    /// `enrich_deprecation` — `#[deprecated]` fact emission.
    Deprecation,
    /// `enrich_bounded_context` — `:Item.bounded_context` re-enrichment.
    BoundedContext,
    /// `enrich_concepts` — `:Concept` node materialization from TOML.
    Concepts,
    /// `enrich_reachability` — BFS from `:EntryPoint` over `CALLS*`.
    Reachability,
    /// `enrich_metrics` — populates `unwrap_count`, `cyclomatic`,
    /// `test_coverage`, `dup_cluster_id` on `:Item{kind:"fn"}` nodes
    /// (producer lands behind the `quality-metrics` feature).
    Metrics,
}

pub fn enrich(
    db: PathBuf,
    keyspace: String,
    verb: EnrichVerb,
    workspace: Option<PathBuf>,
) -> Result<(), crate::CfdbCliError> {
    let (mut store, ks) = compose::load_store_with_workspace(&db, &keyspace, workspace)?;

    let report: EnrichReport = match verb {
        EnrichVerb::GitHistory => store.enrich_git_history(&ks)?,
        // RFC-056 056-A: rfc_docs pass moved to cfdb-enrich::EnrichEngine.
        EnrichVerb::RfcDocs => cfdb_enrich::EnrichEngine::new(&mut store).enrich_rfc_docs(&ks)?,
        // RFC-056: the only verb whose dispatch already moved to
        // EnrichEngine as of slice 056-0 — its "pass" is a trivial
        // extractor-time no-op report, so 056-0 closed its composition-root
        // cutover immediately rather than leaving it unassigned across
        // 056-A..G (no later slice claims this verb).
        EnrichVerb::Deprecation => {
            cfdb_enrich::EnrichEngine::new(&mut store).enrich_deprecation(&ks)?
        }
        // RFC-056 056-B: bounded_context pass moved to cfdb-enrich::EnrichEngine.
        EnrichVerb::BoundedContext => {
            cfdb_enrich::EnrichEngine::new(&mut store).enrich_bounded_context(&ks)?
        }
        EnrichVerb::Concepts => store.enrich_concepts(&ks)?,
        EnrichVerb::Reachability => store.enrich_reachability(&ks)?,
        EnrichVerb::Metrics => store.enrich_metrics(&ks)?,
    };

    // Persist enrichment back to disk when the pass actually ran AND mutated
    // the graph. Phase A stubs (`ran: false`) and no-op reports (`ran: true,
    // attrs_written: 0` — e.g. `enrich_deprecation`, whose real work is
    // extractor-time) skip the save to avoid unnecessary rewrites.
    if report.ran && (report.attrs_written > 0 || report.edges_written > 0) {
        compose::save_store(&store, &ks, &db)?;
    }

    output::emit_json(&report)
}

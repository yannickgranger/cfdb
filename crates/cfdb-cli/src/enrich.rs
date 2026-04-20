//! `cfdb enrich-*` verbs.
//!
//! Split out of `lib.rs` for the god-file decomposition (#3751). Public
//! surface preserved: every item here is re-exported from the crate root.
//!
//! Variant set updated by #43 council round 1 §43-A: renamed the three
//! original stubs to match RFC addendum §A2.2 pass vocabulary and added
//! three new stubs (`BoundedContext`, `Deprecation`, `Reachability`). See
//! [`cfdb_core::enrich::EnrichBackend`] for the trait-level surface and
//! the per-pass slice that ships the real implementation.

use std::path::PathBuf;

use cfdb_core::enrich::{EnrichBackend, EnrichReport};

use crate::compose;

/// Which `enrich_*` verb to dispatch to. Lets one handler function service
/// all CLI variants without duplicating the load-store-print boilerplate.
pub enum EnrichVerb {
    /// `enrich_git_history` — commit age/author/churn per `:Item` (slice 43-B).
    GitHistory,
    /// `enrich_rfc_docs` — `:RfcDoc` + `REFERENCED_BY` emission (slice 43-D).
    RfcDocs,
    /// `enrich_deprecation` — `#[deprecated]` fact emission (slice 43-C,
    /// extractor-time).
    Deprecation,
    /// `enrich_bounded_context` — `:Item.bounded_context` re-enrichment
    /// (slice 43-E + v0.2-9 ≥95% gate).
    BoundedContext,
    /// `enrich_concepts` — `:Concept` node materialization from TOML
    /// (slice 43-F; unblocks #101 + #102).
    Concepts,
    /// `enrich_reachability` — BFS from `:EntryPoint` over `CALLS*`
    /// (slice 43-G).
    Reachability,
    /// `enrich_metrics` — complexity/unwraps/clones-in-loops (deferred
    /// out of #43 scope per RFC addendum §A2.2).
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
        EnrichVerb::RfcDocs => store.enrich_rfc_docs(&ks)?,
        EnrichVerb::Deprecation => store.enrich_deprecation(&ks)?,
        EnrichVerb::BoundedContext => store.enrich_bounded_context(&ks)?,
        EnrichVerb::Concepts => store.enrich_concepts(&ks)?,
        EnrichVerb::Reachability => store.enrich_reachability(&ks)?,
        EnrichVerb::Metrics => store.enrich_metrics(&ks)?,
    };
    let json = serde_json::to_string_pretty(&report)?;
    println!("{json}");
    Ok(())
}

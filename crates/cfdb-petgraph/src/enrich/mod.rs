//! Enrichment pass implementations for [`crate::PetgraphStore`].
//!
//! Each submodule implements one verb from the [`cfdb_core::enrich::EnrichBackend`]
//! trait. The `impl EnrichBackend for PetgraphStore` block in [`crate`] routes
//! calls into these modules under the matching feature gate. Verbs without a
//! real implementation inherit the default `EnrichReport::not_implemented`
//! stubs from the trait.
//!
//! # Slice landings (RFC addendum §A2.2 × #43 council synthesis)
//!
//! | Verb | Module | Slice | Issue | Feature |
//! |---|---|---|---|---|
//! | `enrich_git_history` | [`git_history`] | 43-B | #105 | `git-enrich` |
//!
//! Additional passes (43-D `enrich_rfc_docs`, 43-E `enrich_bounded_context`,
//! 43-F `enrich_concepts`, 43-G `enrich_reachability`) land their modules
//! alongside this one as their slices merge.

// The module is compiled only with the `git-enrich` feature — libgit2 is a
// heavy dep and we gate it per RFC addendum §A2.2 / rust-systems Q1+Q6. The
// feature-off path is handled entirely in `crate::enrich_git_history_impl`
// (no `git2` references → compiles cleanly without the feature).
#[cfg(feature = "git-enrich")]
pub(crate) mod git_history;

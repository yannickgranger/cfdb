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
//! | `enrich_rfc_docs` | [`rfc_docs`] | 43-D | #107 | — |
//! | `enrich_bounded_context` | [`bounded_context`] | 43-E | #108 | — |
//! | `enrich_concepts` | [`concepts`] | 43-F | #109 | — |
//!
//! Remaining pass (43-G `enrich_reachability`) lands its module alongside
//! these as its slice merges.

// The module is compiled only with the `git-enrich` feature — libgit2 is a
// heavy dep and we gate it per RFC addendum §A2.2 / rust-systems Q1+Q6. The
// feature-off path is handled entirely in `crate::enrich_git_history_impl`
// (no `git2` references → compiles cleanly without the feature).
#[cfg(feature = "git-enrich")]
pub(crate) mod git_history;

// Slice 43-D (issue #107) — scans workspace `docs/**/*.md` and
// `.concept-graph/*.md` with stdlib `str::contains` + a hand-rolled word
// boundary check. No feature flag needed: stdlib-only, negligible compile
// cost.
pub(crate) mod rfc_docs;

// Slice 43-E (issue #108) — re-reads `.cfdb/concepts/*.toml` and patches
// `:Item.bounded_context` on items whose crate's mapping changed. Calls
// into the shared `cfdb_concepts` crate so the resolution logic has a
// single home (extract-time + enrich-time cannot diverge).
pub(crate) mod bounded_context;

// Slice 43-F (issue #109) — materialises `:Concept` nodes from
// `.cfdb/concepts/*.toml` and emits `LABELED_AS` + `CANONICAL_FOR` edges.
// Unblocks trigger queries #101 and #102.
pub(crate) mod concepts;

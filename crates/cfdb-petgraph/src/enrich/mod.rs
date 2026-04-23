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
//! | `enrich_reachability` | [`reachability`] | 43-G | #110 | — |
//! | `enrich_metrics` | [`metrics`] | RFC-036 §3.3 | #203 | `quality-metrics` |
//!
//! Phase D enrichment set complete. `enrich_deprecation` ships as an
//! extractor-time fact (slice 43-C / #106) — its EnrichBackend method is a
//! no-op report, not a module in this directory.

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

// Slice 43-G (issue #110) — BFS from every `:EntryPoint` over `CALLS*`
// edges, writing `:Item.reachable_from_entry` + `reachable_entry_count`.
// Degraded path when keyspace has zero entry points: ran=false + warning
// (clean-arch B3). Closes the Phase D enrichment set.
pub(crate) mod reachability;

// Issue #203 / RFC-036 §3.3 — populates previously-reserved
// `EnrichMetrics`-provenance attrs (`unwrap_count`, `cyclomatic`,
// `test_coverage`, `dup_cluster_id`) on `:Item{kind:"Fn"}` by re-parsing
// source files with `syn` (stateless full re-walk). Gated behind
// `quality-metrics` so default builds avoid syn's parse cost on large
// target workspaces. DIP constraint (RFC-036 CP6): parses syn directly;
// dep direction `cfdb-petgraph → cfdb-extractor` is forbidden.
#[cfg(feature = "quality-metrics")]
pub(crate) mod metrics;

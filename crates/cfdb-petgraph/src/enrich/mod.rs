//! Enrichment pass implementations for [`crate::PetgraphStore`].
//!
//! Each submodule implements one verb from the [`cfdb_core::enrich::EnrichBackend`]
//! trait. The `impl EnrichBackend for PetgraphStore` block in [`crate`] routes
//! calls into these modules under the matching feature gate. Verbs without a
//! real implementation inherit the default `EnrichReport::not_implemented`
//! stubs from the trait.
//!
//! # Slice landings
//!
//! | Verb | Module | Slice | Issue | Feature |
//! |---|---|---|---|---|
//! | `enrich_reachability` | [`reachability`] | — | — | — |
//! | `enrich_metrics` | [`metrics`] | — | — | `quality-metrics` |
//!
//! `enrich_deprecation` ships as an extractor-time fact — its EnrichBackend
//! method is a no-op report, not a module in this directory. `enrich_rfc_docs`
//! moved to `cfdb-enrich::rfc_docs` (RFC-056 slice 056-A). `enrich_bounded_context`
//! moved to `cfdb-enrich::bounded_context` (RFC-056 slice 056-B). `enrich_concepts`
//! moved to `cfdb-enrich::concepts` (RFC-056 slice 056-C). `enrich_git_history`
//! moved to `cfdb-enrich::git_history` (RFC-056 slice 056-D) — this crate's
//! own `git-enrich` feature (and its `git2` optional dep) remains declared
//! but now unused, pruned at 056-G once all 7 verbs have moved.

// BFS from every `:EntryPoint` over `CALLS*` edges, writing
// `:Item.reachable_from_entry` + `reachable_entry_count`. Degraded path when
// keyspace has zero entry points: ran=false + warning.
pub(crate) mod reachability;

// Recall post-pass folded into `reachability::run`. Flips
// `reachable_from_entry = true` on fn `:Item`s referenced by a
// `:CallSite{kind="serde_default"}` whose `callee_path` resolves to a
// workspace-known qname — handles the false-positive class where serde's
// derived `Deserialize` impl invokes a default fn via proc-macro expansion
// that cfdb cannot trace.
pub(crate) mod attr_call_resolution;

// Populates previously-reserved `EnrichMetrics`-provenance attrs
// (`unwrap_count`, `cyclomatic`, `test_coverage`, `dup_cluster_id`) on
// `:Item{kind:"Fn"}` by re-parsing source files with `syn` (stateless full
// re-walk). Gated behind `quality-metrics` so default builds avoid syn's
// parse cost on large target workspaces.
#[cfg(feature = "quality-metrics")]
pub(crate) mod metrics;

//! Enrichment pass implementations for [`crate::PetgraphStore`].
//!
//! Each submodule implements one verb from the [`cfdb_core::enrich::EnrichBackend`]
//! trait; `impl EnrichBackend for PetgraphStore` in [`crate`] routes calls
//! into these modules. Verbs without a real implementation here inherit the
//! default `EnrichReport::not_implemented` stub from the trait.
//!
//! `enrich_deprecation` ships as an extractor-time fact — its EnrichBackend
//! method is a no-op report, not a module in this directory.

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

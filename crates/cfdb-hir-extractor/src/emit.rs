//! The `CallSiteEmitter` trait — orphan-rule-safe bridge between the
//! HIR extractor's output facts and a concrete store implementation.
//!
//! ## Why this trait lives HERE (not in `cfdb-core`, not in a store crate)
//!
//! RFC-032 §3 lines 229–242: `cfdb-hir-extractor` defines its own
//! extraction traits; store-adapter crates (e.g.,
//! `cfdb-hir-petgraph-adapter`) implement them. Placing the trait in
//! this crate keeps the orphan rule satisfied without requiring
//! `cfdb-core` (the innermost layer) to know about HIR concerns, and
//! without forcing store crates (`cfdb-petgraph`) to depend on any
//! `ra-ap-*` crate — that dependency is quarantined to the adapter
//! crate.
//!
//! ## Why the trait is HirDatabase-agnostic
//!
//! The trait signature takes `(Vec<Node>, Vec<Edge>)` — pre-extracted
//! facts — not a `HirDatabase` handle. This is deliberate:
//!
//! - The HIR extraction (slice 3c, Issue #85c) consumes a
//!   `ra_ap_hir::db::HirDatabase` monomorphically and produces
//!   `(Vec<Node>, Vec<Edge>)` via a free function
//!   (`extract_call_sites::<DB>`). That function will be the sole
//!   bridge between `ra-ap-hir` types and `cfdb-core` vocabulary.
//!
//! - The `CallSiteEmitter` trait then routes those pre-extracted
//!   facts into a store. Since it never sees `ra-ap-*` types, every
//!   store crate can implement it without pulling the 90–150s
//!   compile cost (RFC-032 §3 lines 221–227).
//!
//! - ISP: `StoreBackend` (in `cfdb-core`) exposes many concerns
//!   (ingest, execute, canonical-dump, enrich). `CallSiteEmitter`
//!   narrows the surface to exactly the HIR-emission concern — the
//!   only one adapter callers need.
//!
//! ## Slice status
//!
//! Trait and `EmitStats` shipped in slice 3b (Issue #92). The first
//! adapter impl lives in `cfdb-hir-petgraph-adapter`. The HIR
//! extraction function `extract_call_sites` that produces the inputs
//! is deferred to slice 3c (Issue #85c).

use cfdb_core::fact::{Edge, Node};

/// Ingest pre-extracted HIR-resolved call-site facts into a store.
///
/// Implementors accept the `(nodes, edges)` tuple produced by the
/// (slice 3c) `extract_call_sites` function and route it into
/// whatever backing store they wrap. The trait reports a structured
/// `EmitStats` — the number of `:CallSite` nodes, `CALLS` edges, and
/// `INVOKES_AT` edges ingested — so callers can assert on the count
/// without re-scanning the returned fact lists.
///
/// Contract: implementors MUST NOT filter out nodes/edges based on
/// label. If an input contains non-HIR facts (e.g., a stray `:Item`
/// emission), the implementor MAY reject the batch with an error or
/// MAY pass it through; both are permitted. The `EmitStats` counts
/// reflect only the three HIR-related labels regardless.
pub trait CallSiteEmitter {
    /// Error produced by the underlying store when ingestion fails.
    type Err;

    /// Ingest a batch of resolved call-site facts.
    fn ingest_resolved_call_sites(
        &mut self,
        nodes: Vec<Node>,
        edges: Vec<Edge>,
    ) -> Result<EmitStats, Self::Err>;
}

/// Observable counts returned by `CallSiteEmitter::ingest_resolved_call_sites`.
///
/// Every field is a cardinality (non-negative). Counts reflect the
/// input batch — NOT the cumulative store state. Callers that need
/// cumulative totals must aggregate across successive calls.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EmitStats {
    /// Count of nodes with `label == Label::CALL_SITE` in the input.
    pub call_sites_emitted: usize,
    /// Count of edges with `label == EdgeLabel::CALLS` in the input.
    pub calls_edges_emitted: usize,
    /// Count of edges with `label == EdgeLabel::INVOKES_AT` in the input.
    pub invokes_at_edges_emitted: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emit_stats_default_is_all_zero() {
        let s = EmitStats::default();
        assert_eq!(s.call_sites_emitted, 0);
        assert_eq!(s.calls_edges_emitted, 0);
        assert_eq!(s.invokes_at_edges_emitted, 0);
    }

    #[test]
    fn emit_stats_equality_is_field_wise() {
        let a = EmitStats {
            call_sites_emitted: 3,
            calls_edges_emitted: 2,
            invokes_at_edges_emitted: 3,
        };
        let b = EmitStats {
            call_sites_emitted: 3,
            calls_edges_emitted: 2,
            invokes_at_edges_emitted: 3,
        };
        assert_eq!(a, b);
    }
}

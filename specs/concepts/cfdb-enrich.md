# Spec: cfdb-enrich

The strangler-fig destination for `cfdb-petgraph`'s 7 enrichment passes (RFC-056). Depends on `cfdb-core` (production) plus `cfdb-concepts` (056-B onward, for passes that resolve `.cfdb/concepts/*.toml`) — never on `cfdb-petgraph` or any concrete storage engine, so an enrichment pass can never reach into a backend's internal graph representation by accident. `EnrichEngine` implements `enrich_deprecation` (056-0), `enrich_rfc_docs` (056-A), `enrich_bounded_context` (056-B), and `enrich_concepts` (056-C, the slice that proved `ingest_nodes`/`ingest_edges` — the node-creation path — through the port); the remaining three fall through to `EnrichBackend`'s default stubs until their slice (056-D through 056-F) moves them in. As of 056-C, `cfdb-petgraph` no longer depends on `cfdb-concepts` at all — its only two consumers (`bounded_context`, `concepts`) have both moved out.

Test suites depend on `cfdb-petgraph` in `[dev-dependencies]` only (the moved passes' own tests need a concrete `GraphBackend` to run against, and `PetgraphStore` is the only one that exists) — exempt from the CLEAN-3 dependency-rule gate, which scans `[dependencies]` only.

## EnrichEngine

Wraps any `GraphBackend` implementor (borrowed, not owned — mirrors `PetgraphStore`'s own enrich dispatch) and implements `cfdb_core::enrich::EnrichBackend` generically over it. Pure dispatch plus the two guards every verb needs (keyspace existence — delegated to `GraphBackend::graph_view`; workspace-root presence — `require_workspace`, moved verbatim from `cfdb-petgraph::enrich_backend.rs` so a migrated pass's warning text stays byte-identical). No pass-level logic lives here by design — that would recreate, in miniature, the bundling problem RFC-056 fixes at the crate level.

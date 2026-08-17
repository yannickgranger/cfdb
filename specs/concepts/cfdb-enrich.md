# Spec: cfdb-enrich

The strangler-fig destination for `cfdb-petgraph`'s 7 enrichment passes (RFC-056). Depends only on `cfdb-core` — never on `cfdb-petgraph` or any concrete storage engine, so an enrichment pass can never reach into a backend's internal graph representation by accident. As of slice 056-0 (the scaffold), no pass has moved yet; `EnrichEngine` implements only `enrich_deprecation` and falls through to `EnrichBackend`'s default stubs for the other six. Slices 056-A through 056-F each move one pass in.

Test suites depend on `cfdb-petgraph` in `[dev-dependencies]` only (the moved passes' own tests need a concrete `GraphBackend` to run against, and `PetgraphStore` is the only one that exists) — exempt from the CLEAN-3 dependency-rule gate, which scans `[dependencies]` only.

## EnrichEngine

Wraps any `GraphBackend` implementor (borrowed, not owned — mirrors `PetgraphStore`'s own enrich dispatch) and implements `cfdb_core::enrich::EnrichBackend` generically over it. Pure dispatch plus the two guards every verb needs (keyspace existence — delegated to `GraphBackend::graph_view`; workspace-root presence — `require_workspace`, moved verbatim from `cfdb-petgraph::enrich_backend.rs` so a migrated pass's warning text stays byte-identical). No pass-level logic lives here by design — that would recreate, in miniature, the bundling problem RFC-056 fixes at the crate level.

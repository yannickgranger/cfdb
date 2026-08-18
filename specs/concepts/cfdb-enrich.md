# Spec: cfdb-enrich

Implements `cfdb-petgraph`'s enrichment passes behind the `GraphBackend`/`GraphView` port. Depends on `cfdb-core`, `cfdb-concepts` (for passes resolving `.cfdb/concepts/*.toml`), feature-gated `git2` (`git-enrich`), and feature-gated `syn`/`sha2` (`quality-metrics`) — never on `cfdb-petgraph` or any concrete storage engine, so an enrichment pass can never reach into a backend's internal graph representation.

`EnrichEngine` implements all seven `EnrichBackend` verbs: `enrich_deprecation`, `enrich_rfc_docs`, `enrich_bounded_context`, `enrich_concepts`, `enrich_git_history`, `enrich_metrics`, and `enrich_reachability` (BFS from every `:EntryPoint` over `CALLS`/`INVOKES_AT`, All then ProductionOnly pass, plus the `#[serde(default = "fn")]` callee post-pass).

Test suites depend on `cfdb-petgraph` in `[dev-dependencies]` only (a concrete `GraphBackend` to run against, and `PetgraphStore` is the only one that exists) — exempt from the CLEAN-3 dependency-rule gate, which scans `[dependencies]` only.

## EnrichEngine

Wraps any `GraphBackend` implementor (borrowed, not owned) and implements `cfdb_core::enrich::EnrichBackend` generically over it. Pure dispatch plus the two guards every verb needs (keyspace existence via `GraphBackend::graph_view`; workspace-root presence via `require_workspace`). No pass-level logic lives here — that responsibility stays with each pass module.

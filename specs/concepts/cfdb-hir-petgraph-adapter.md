# Spec: cfdb-hir-petgraph-adapter

Orphan-rule-safe bridge between `cfdb-hir-extractor`'s `CallSiteEmitter` trait and `cfdb-petgraph`'s `PetgraphStore` type. The impl lives in a dedicated crate (not in `cfdb-petgraph`) because `cfdb-cli → cfdb-petgraph` already exists — placing the impl inside `cfdb-petgraph` would transitively contaminate `cfdb-cli` with the 90–150s `ra-ap-*` cold-compile cost on every default build, violating RFC-032 §3 lines 221–227. This adapter is pulled into `cfdb-cli` only via the `hir` feature flag (Issue #86 / slice 4).

The adapter contains no HIR-type handling — all `ra-ap-*` interaction stays inside `cfdb-hir-extractor`. An architecture test (`tests/arch_cli_isolation.rs`) enforces both invariants: no direct `ra-ap-*` reference in this crate's sources, and no transitive `cfdb-cli → cfdb-hir-*` / `ra-ap-*` dependency arrow.

## PetgraphAdapter

Pairs a mutable `&mut PetgraphStore` with a target `Keyspace` and implements `CallSiteEmitter` against that pair. Ingestion counts `:CallSite`, `CALLS`, and `INVOKES_AT` labels in the input batch, routes all supplied nodes and edges into the store via `StoreBackend::ingest_nodes` / `ingest_edges`, and returns `EmitStats` with the counted cardinalities. Keyspace is created lazily on first ingest if absent.

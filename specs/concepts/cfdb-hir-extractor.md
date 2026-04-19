# Spec: cfdb-hir-extractor

The HIR-backed companion to the syn-based `cfdb-extractor`. Consumes a monomorphic `ra_ap_hir::db::HirDatabase` and emits resolved `:CallSite`, `CALLS`, `INVOKES_AT`, and (v0.2+ complete) `:EntryPoint` facts into `cfdb-core`'s schema vocabulary. `ra-ap-*` crates are quarantined inside this crate — RFC-032 §3 boundary test enforces zero `ra_ap_*` references in `cfdb-core`.

v0.2 adoption is incremental (RFC-032 §3 / Group C, issue #40): slice 2 (#84) shipped the scaffold + boundary architecture test; slice 3b (#92) adds the `CallSiteEmitter` trait; slice 3c (#85c) adds the `build_hir_database` + `extract_call_sites` free functions that produce the facts; slice 4 (#86) adds `:EntryPoint` emission and wires the CLI behind a `hir` feature flag.

## CallSiteEmitter

Trait defining the store-adapter contract: consume pre-extracted `(Vec<Node>, Vec<Edge>)` resolved-call-site facts and route them into a backing store, returning structured `EmitStats`. The trait is HirDatabase-agnostic — it takes only `cfdb-core` vocabulary types. HIR loading and extraction live in free functions outside the trait so implementor crates (e.g. `cfdb-hir-petgraph-adapter`) do not inherit the 90–150s `ra-ap-*` compile cost unless they explicitly opt into the extractor. See RFC-032 §3 lines 229–242 for the orphan-rule placement rationale.

## EmitStats

Observable counts returned by `CallSiteEmitter::ingest_resolved_call_sites`: `call_sites_emitted`, `calls_edges_emitted`, `invokes_at_edges_emitted`. Counts reflect the input batch, not cumulative store state — callers aggregate across successive calls when they need totals. Default-constructed `EmitStats` is all-zero.

# Spec: cfdb-hir-extractor

The HIR-backed companion to the syn-based `cfdb-extractor`. Consumes a monomorphic `ra_ap_hir::db::HirDatabase` and emits resolved `:CallSite`, `CALLS`, `INVOKES_AT`, and (v0.2+ complete) `:EntryPoint` facts into `cfdb-core`'s schema vocabulary. `ra-ap-*` crates are quarantined inside this crate — RFC-032 §3 boundary test enforces zero `ra_ap_*` references in `cfdb-core`.

v0.2 adoption is incremental (RFC-032 §3 / Group C, issue #40): slice 2 (#84) shipped the scaffold + boundary architecture test; slice 3b (#92) adds the `CallSiteEmitter` trait; slice 3c (#85c) adds the `build_hir_database` + `extract_call_sites` free functions that produce the facts; slice 4 (#86) adds `:EntryPoint` emission and wires the CLI behind a `hir` feature flag.

## CallSiteEmitter

Trait defining the store-adapter contract: consume pre-extracted `(Vec<Node>, Vec<Edge>)` resolved-call-site facts and route them into a backing store, returning structured `EmitStats`. The trait is HirDatabase-agnostic — it takes only `cfdb-core` vocabulary types. HIR loading and extraction live in free functions outside the trait so implementor crates (e.g. `cfdb-hir-petgraph-adapter`) do not inherit the 90–150s `ra-ap-*` compile cost unless they explicitly opt into the extractor. See RFC-032 §3 lines 229–242 for the orphan-rule placement rationale.

## EmitStats

Observable counts returned by `CallSiteEmitter::ingest_resolved_call_sites`: `call_sites_emitted`, `calls_edges_emitted`, `invokes_at_edges_emitted`, `entry_points_emitted`, `exposes_edges_emitted` (five fields since Issue #86 added the entry-point emitter). Counts reflect the input batch, not cumulative store state — callers aggregate across successive calls when they need totals. Default-constructed `EmitStats` is all-zero.

Entry-point emission (landed via #86 / #124 / #125) produces `:EntryPoint` nodes in one of five `kind` strings — `mcp_tool`, `cli_command`, `http_route`, `cron_job`, `websocket` — with an `EXPOSES` edge from the `:EntryPoint` to its handler `:Item{kind:fn|method}`. The `:EntryPoint` lifecycle is coupled to the handler per RFC-036 CP4: no standalone `:EntryPoint` without a corresponding handler `:Item`. `REGISTERS_PARAM` emission (RFC-036 §3.1, tracked in #201) extends this emitter by adding edges from each `:EntryPoint` to the `:Param` nodes its handler declares via `HAS_PARAM` — same `:Param` node id formula (`cfdb-core::qname::param_node_id`), distinct edge semantics. HTTP route handler params are deferred to v3 because they require HIR fn-signature resolution of the extracted function; v2 `http_route` `:EntryPoint` nodes emit an empty `params` list.


## HirError

Error type produced by the HIR pipeline. Covers workspace-discovery failures (`ProjectManifest` cannot be located), loader failures (`load_workspace_at` returned an error from `ra_ap_load_cargo`), and parse failures during syntax-tree walking. Variants carry the offending `PathBuf` and a stringified message — NO `ra_ap_*` concrete type ever appears in the error payload, preserving the RFC-029 §A1.2 boundary contract (acceptance gate v0.2-6).

The three public free functions — `build_hir_database(workspace_root) -> Result<(RootDatabase, Vfs), HirError>`, `extract_call_sites<DB: HirDatabase + Sized>(db, vfs) -> Result<(Vec<Node>, Vec<Edge>), HirError>`, and `extract_entry_points<DB>(db, vfs) -> Result<(Vec<Node>, Vec<Edge>), HirError>` — compose to emit the full v0.2+ HIR-enriched fact set into cfdb-core's schema vocabulary. Extraction is deterministic (output sorted by ID) and wraps work in `ra_ap_hir_ty::attach_db` to satisfy hir-ty's next-solver thread-local requirement. Unresolved individual calls are silently skipped — the syn extractor catches those as `resolver="syn"`. The SchemaVersion v0.1.4 bump introduced `CALLS` edges with a `resolved: bool` attribute; v0.2.0 (Issue #86) added `:EntryPoint` nodes and `EXPOSES` edges. `REGISTERS_PARAM` emission (RFC-036 §3.1, tracked in #201) extends `extract_entry_points` to also emit `REGISTERS_PARAM` edges into the `:Param` nodes produced by `cfdb-extractor`'s `HAS_PARAM` pass (Issue #209).

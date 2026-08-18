# Spec: cfdb-enrich

Implements `cfdb-petgraph`'s enrichment passes behind the `GraphBackend`/`GraphView` port. Depends on `cfdb-core`, `cfdb-concepts` (for passes resolving `.cfdb/concepts/*.toml`), feature-gated `git2` (`git-enrich`), and feature-gated `syn`/`sha2` (`quality-metrics`) — never on `cfdb-petgraph` or any concrete storage engine, so an enrichment pass can never reach into a backend's internal graph representation.

`EnrichEngine` implements all seven `EnrichBackend` verbs: `enrich_deprecation`, `enrich_rfc_docs`, `enrich_bounded_context`, `enrich_concepts`, `enrich_git_history`, `enrich_metrics`, and `enrich_reachability` (BFS from every `:EntryPoint` over `CALLS`/`INVOKES_AT`, All then ProductionOnly pass, plus the `#[serde(default = "fn")]` callee post-pass).

Test suites depend on `cfdb-petgraph` in `[dev-dependencies]` only (a concrete `GraphBackend` to run against, and `PetgraphStore` is the only one that exists) — exempt from the CLEAN-3 dependency-rule gate, which scans `[dependencies]` only.

## EnrichEngine

Wraps any `GraphBackend` implementor (borrowed, not owned) and implements `cfdb_core::enrich::EnrichBackend` generically over it. Pure dispatch plus the two guards every verb needs (keyspace existence via `GraphBackend::graph_view`; workspace-root presence via `require_workspace`). No pass-level logic lives here — that responsibility stays with each pass module.

## AstSignals

Per-function AST-derived signal pair: `{ unwrap_count, cyclomatic }`. Produced by `cfdb_enrich::metrics::ast_signals` when the `quality-metrics` feature is active. `unwrap_count` counts `.unwrap()` + `.expect()` method calls in the function body; `cyclomatic` is McCabe complexity (branches + 1) counting `if` / `match` (N arms → N−1) / loops / `?` / `&&` / `||`. Stateless full re-walk of every distinct source file referenced by a `:Item{kind:"Fn"}.file` prop — no incremental-parse mode. Parses via `syn` directly inside `cfdb-enrich`; the dep direction `cfdb-enrich → cfdb-extractor` is forbidden.

## Config

Per-run configuration for `enrich_metrics` (`metrics::Config`). One field: `coverage_json: Option<PathBuf>` naming a `cargo llvm-cov --json` output file. `None` leaves `:Item.test_coverage` unpopulated; `Some` populates per-qname from the file's `summary.lines.percent` block. `Default::default()` yields `coverage_json: None` — matches the G6 invariant (test_coverage toolchain-version-scoped, excluded from G1 canonical-dump sha256). Internal producer helpers (`compute_for_block`, `compute_for_item`, `compute_dup_cluster_ids`, `hash_cluster`, `parse_llvm_cov_json`) are `pub(crate)` and therefore not separately catalogued.

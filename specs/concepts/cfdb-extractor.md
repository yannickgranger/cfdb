# Spec: cfdb-extractor

The Rust-source-to-facts extractor — walks a cargo workspace via `syn` and `cargo_metadata` and emits `Node` / `Edge` values for ingest. Depends on `cfdb-core` for the emit-side types; no other workspace dependency.

v0.2 adds `cfdb-hir-extractor` as a parallel crate for HIR-backed facts (RFC-032 §3 / Group C, issue #40). The two extractors share `cfdb-core`'s schema vocabulary but are otherwise independent.

## ExtractError

Error type produced during workspace walking — covers cargo-metadata failures, I/O on source files, and `syn` parse errors. Propagated up to the caller (typically `cfdb extract`) which formats it for the user.

## RustExtractPhases

Per-phase wall-clock breakdown of one `extract_workspace_profiled` run, attributing the extractor's three internal phases — the `cargo metadata` subprocess, the `syn` walk (concept-override load, crate-tier DAG, per-file parse and visit, context emission), and the post-walk deferred resolution (RETURNS / TYPE_OF resolvers, referenced-item synthesis, canonical sort). Named for RFC-048 §1's profile gate, which measures where `cfdb extract` spends its seconds so the operator can decide whether the deferred incremental-extraction slices are worth filing. This is build-process telemetry, not schema vocabulary: it carries no node label, no `:Item` attribute, and never reaches the emitted facts or their determinism hash (RFC-048 §4). Produced only by the profiled entry point; the un-profiled `extract_workspace` discards it, so both share one code path and the profiled run's `(nodes, edges)` output stays byte-identical to a plain extract.

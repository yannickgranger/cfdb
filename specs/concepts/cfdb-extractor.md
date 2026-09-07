# cfdb-extractor

The Rust-source-to-facts extractor — walks a cargo workspace via `syn` and `cargo_metadata` and emits `Node` / `Edge` values for ingest. Depends on `cfdb-core` for the emit-side types; no other workspace dependency.

v0.2 adds `cfdb-hir-extractor` as a parallel crate for HIR-backed facts (RFC-032 §3 / Group C, issue #40). The two extractors share `cfdb-core`'s schema vocabulary but are otherwise independent.

## ExtractError

Error type produced during workspace walking — covers cargo-metadata failures, I/O on source files, and `syn` parse errors. Propagated up to the caller (typically `cfdb extract`) which formats it for the user.

## ExtractPhaseMarker

Phase-transition marker the profiled extract entry point emits to an observer at each boundary between the extractor's three internal phases — before the `cargo metadata` subprocess, before the `syn` walk (concept-override load, crate-tier DAG, per-file parse and visit, context emission), before the post-walk deferred resolution (RETURNS / TYPE_OF resolvers, referenced-item synthesis, canonical sort), and after extraction finishes. Named for RFC-048 §1's profile gate, which measures where `cfdb extract` spends its seconds so the operator can decide whether the deferred incremental-extraction slices are worth filing. Pure control-flow signal, never schema vocabulary and never a clock read: RFC-029 §12.1 G1 forbids wall-clock reads in this crate (enforced by `tests/architecture_determinism.rs`), so the extractor emits only the boundaries and the composition root (cfdb-cli) owns the clock — it timestamps each marker and derives the per-phase durations. The markers carry no node label, no `:Item` attribute, and never reach the emitted facts or their determinism hash (RFC-048 §4). The un-profiled `extract_workspace` delegates to the profiled entry with a no-op observer, so both share one code path and the profiled run's `(nodes, edges)` output stays byte-identical to a plain extract.

# Spec: cfdb-extractor

The Rust-source-to-facts extractor — walks a cargo workspace via `syn` and `cargo_metadata` and emits `Node` / `Edge` values for ingest. Depends on `cfdb-core` for the emit-side types; no other workspace dependency.

v0.2 adds `cfdb-hir-extractor` as a parallel crate for HIR-backed facts (RFC-032 §3 / Group C, issue #40). The two extractors share `cfdb-core`'s schema vocabulary but are otherwise independent.

## ExtractError

Error type produced during workspace walking — covers cargo-metadata failures, I/O on source files, and `syn` parse errors. Propagated up to the caller (typically `cfdb extract`) which formats it for the user.

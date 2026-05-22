//! Fixture for RFC-042 042-A — test/bench `:EntryPoint` extraction.
//!
//! `src/lib.rs` contains a single production fn that is the CONTROL row
//! (must NOT emit an `:EntryPoint`). Test/bench surfaces live in
//! `tests/*.rs` and `benches/*.rs` — Cargo integration-test and bench
//! target conventions.

/// Plain library fn — production code. Must NOT emit `:EntryPoint`
/// (no `#[tool]`, no `#[test]`, no `#[bench]`, lives under `src/` not
/// `tests/`).
pub fn library_fn() {}

//! `dogfood-enrich` — RFC-039 self-dogfood harness for the 7 enrichment
//! passes. Standalone leaf binary; subprocess-driven (does NOT link
//! `cfdb-cli` as a library). See `docs/RFC-039-dogfood-enrichment-passes.md`.
//!
//! The library surface exists for unit-testing the pure helpers
//! (template substitution, EnrichReport parsing, threshold lookup).
//! The binary in `src/main.rs` is the CI entry point.

pub mod count_items;
pub mod feature_guard;
pub mod grep_deprecated;
pub mod grep_rfc_docs;
pub mod passes;
pub mod runner;
pub mod scan_concepts;
pub mod thresholds;

/// Exit code on zero violation rows.
pub const EXIT_OK: i32 = 0;

/// Exit code on at least one violation row (RFC-033 §3.2 / cfdb-cli
/// `Violations` exit-30 contract).
pub const EXIT_VIOLATIONS: i32 = 30;

/// Exit code on runtime error (subprocess failure, missing template,
/// JSON parse failure, missing feature).
pub const EXIT_RUNTIME_ERROR: i32 = 1;

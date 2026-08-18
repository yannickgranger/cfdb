//! cfdb-eval — the Cypher-subset evaluator, behind the `GraphReader` port.
//!
//! [`QueryEngine`] implements [`cfdb_core::store::QueryBackend`] generically
//! over any [`cfdb_core::graph::GraphBackend`] implementor, so this crate
//! never depends on `cfdb-petgraph` (or any concrete storage engine) in
//! production — only in `[dev-dependencies]`, for the evaluator test
//! suites, which need a concrete backend to run against.
//!
//! cfdb-core schema enums are `#[non_exhaustive]`. Cross-crate `match` sites
//! on those enums require a `_ =>` arm by hard compile error (E0004). The
//! `non_exhaustive_omitted_patterns` lint further tightens this at the
//! wildcard-arm boundary; we deny it preemptively so the attribute
//! auto-activates when the lint stabilises (currently nightly-only;
//! `allow(unknown_lints)` keeps the attribute inert on stable).

#![allow(unknown_lints)]
#![deny(non_exhaustive_omitted_patterns)]

mod engine;
mod eval;
pub mod explain;

pub use engine::QueryEngine;

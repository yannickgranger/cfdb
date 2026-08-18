//! cfdb-classify — the judgment layer over cfdb's code facts.
//!
//! Two bounded contexts share this crate and never import from each other:
//! **debt classification** (the six-class [`DebtClass`] taxonomy, [`Finding`]
//! rows, the [`ScopeInventory`] / [`ClassifyEnvelope`] wire envelopes and the
//! classifier rules behind `scope` / `classify`) and the **editorial-drift
//! triggers** (the closed [`TriggerId`] registry behind `check`, reporting a
//! [`CheckReport`]). [`ClassifyEngine`] is the shared entry point that
//! dispatches to either. Which skill acts on a `DebtClass` is the consumer's
//! decision, kept outside this repository (`tests/finding_no_skill_field.rs`).
//!
//! The engine reaches a keyspace only through `cfdb_core::graph::GraphBackend`
//! (via `cfdb_eval::QueryEngine`) and never does I/O; the composition root
//! loads the store, prints, writes files and exits.
//!
//! cfdb-core schema enums are `#[non_exhaustive]`. Cross-crate `match` sites
//! on those enums require a `_ =>` arm by hard compile error (E0004). The
//! `non_exhaustive_omitted_patterns` lint further tightens this at the
//! wildcard-arm boundary; we deny it preemptively so the attribute
//! auto-activates when the lint stabilises (`allow(unknown_lints)` keeps the
//! attribute inert on stable).

#![allow(unknown_lints)]
#![deny(non_exhaustive_omitted_patterns)]

mod check;
pub mod classify;
mod engine;
pub mod explain;
mod rules;
mod scope;
pub mod taxonomy;

pub use check::{CheckReport, TriggerId, UnknownTriggerId};
pub use classify::{ClassifyEnvelope, DiffSourceMeta, CLASSIFY_ENVELOPE_SCHEMA_VERSION};
pub use engine::{ClassifyEngine, ClassifyError, ScopeOptions};
pub use explain::ExplainSink;
pub use taxonomy::{
    CanonicalCandidate, DebtClass, Finding, ReachabilityEntry, ScopeInventory, UnknownDebtClass,
};

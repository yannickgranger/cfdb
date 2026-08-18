//! cfdb-classify — the judgment layer over cfdb's code facts.
//!
//! Two bounded contexts share this crate: **debt classification** (the
//! six-class [`DebtClass`] taxonomy, [`Finding`] rows, the [`ScopeInventory`]
//! and [`ClassifyEnvelope`] wire envelopes) and, once the engine lands, the
//! **editorial-drift triggers** (`check`). They never import from each other.
//! Which skill acts on a `DebtClass` is the consumer's decision, kept outside
//! this repository (`tests/finding_no_skill_field.rs`).
//!
//! cfdb-core schema enums are `#[non_exhaustive]`. Cross-crate `match` sites
//! on those enums require a `_ =>` arm by hard compile error (E0004). The
//! `non_exhaustive_omitted_patterns` lint further tightens this at the
//! wildcard-arm boundary; we deny it preemptively so the attribute
//! auto-activates when the lint stabilises (`allow(unknown_lints)` keeps the
//! attribute inert on stable).

#![allow(unknown_lints)]
#![deny(non_exhaustive_omitted_patterns)]

pub mod classify;
pub mod taxonomy;

pub use classify::{ClassifyEnvelope, DiffSourceMeta, CLASSIFY_ENVELOPE_SCHEMA_VERSION};
pub use taxonomy::{
    CanonicalCandidate, DebtClass, Finding, ReachabilityEntry, ScopeInventory, UnknownDebtClass,
};

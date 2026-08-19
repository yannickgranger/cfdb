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

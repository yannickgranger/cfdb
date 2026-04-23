//! Core ingest / query / dump command handlers.
//!
//! Split out of `lib.rs` for the god-file decomposition (#3751); further
//! split into submodules for the drift god-file decomposition (#151).
//! Public surface preserved: every item here is re-exported from the
//! crate root.

mod aux;
mod classify;
mod diff;
mod extract;
mod query;
mod rules;

#[cfg(test)]
mod tests;

pub use aux::{dump, export, list_keyspaces};
pub use classify::classify;
pub use diff::diff;
pub use extract::{extract, keyspace_path};
pub use query::{list_callers, query};
pub use rules::violations;

pub(crate) use rules::parse_and_execute;

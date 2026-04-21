//! cfdb-cli — command implementations.
//!
//! The binary entry point in `src/main.rs` is a thin dispatcher that
//! parses CLI args via clap and calls into the functions here. This
//! split exists so other cfdb crates (and integration tests) can call
//! command logic directly without spawning a subprocess.
//!
//! The implementation is split across sibling modules for file-size
//! hygiene (#3751); every item is re-exported at the crate root so the
//! public API surface is unchanged.

mod check;
mod check_predicate;
mod commands;
mod compose;
mod enrich;
mod error;
#[cfg(feature = "hir")]
mod hir;
mod param_resolver;
mod scope;
mod stubs;

pub use check::{check, TriggerId, UnknownTriggerId};
pub use check_predicate::{check_predicate, PredicateRow, PredicateRunReport};
pub use commands::{
    dump, export, extract, keyspace_path, list_callers, list_keyspaces, query, violations,
};

pub use enrich::{enrich, EnrichVerb};
pub use error::CfdbCliError;
#[cfg(feature = "hir")]
pub use hir::{extract_and_ingest_hir, HirExtractError};
pub use scope::scope;
pub use stubs::{
    diff, drop_keyspace_cmd, list_items_matching, schema_describe_cmd, snapshots, typed_stub,
};

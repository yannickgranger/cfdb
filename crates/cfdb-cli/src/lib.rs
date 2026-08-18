//! cfdb-cli — command implementations.
//!
//! The binary entry point in `src/main.rs` is a thin dispatcher that
//! parses CLI args via clap and calls into the functions here. This
//! split exists so other cfdb crates (and integration tests) can call
//! command logic directly without spawning a subprocess.
//!
//! The implementation is split across sibling modules for file-size
//! hygiene; every item is re-exported at the crate root so the public
//! API surface is unchanged.
//!
//! cfdb-core schema enums are `#[non_exhaustive]`. Cross-crate `match`
//! sites require `_ =>` arms by E0004; the deny below auto-activates when
//! `non_exhaustive_omitted_patterns` stabilises (nightly only on rust 1.93).

#![allow(unknown_lints)]
#![deny(non_exhaustive_omitted_patterns)]

#[cfg(feature = "classify")]
mod check;
mod check_predicate;
mod commands;
mod compose;
mod enrich;
mod error;
#[cfg(feature = "hir")]
mod hir;
mod lang;
mod output;
mod param_resolver;
mod profile;
#[cfg(feature = "classify")]
mod scope;
mod stubs;

#[cfg(feature = "classify")]
pub use check::check;
pub use check_predicate::{check_predicate, PredicateRow, PredicateRunReport};
#[cfg(feature = "classify")]
pub use commands::classify;
pub use commands::{
    diff, dump, export, extract, impact, keyspace_path, list_callers, list_keyspaces, query,
    violations,
};

pub use enrich::{enrich, EnrichVerb};
pub use error::CfdbCliError;
#[cfg(feature = "hir")]
pub use hir::{extract_and_ingest_hir, HirExtractError};
pub use output::{emit_json, OutputFormat};
pub use profile::ExtractProfile;
#[cfg(feature = "classify")]
pub use scope::scope;
pub use stubs::{
    drop_keyspace_cmd, list_items_matching, schema_describe_cmd, snapshots, typed_stub,
};

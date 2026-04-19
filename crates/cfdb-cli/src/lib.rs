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

mod commands;
mod enrich;
mod error;
mod scope;
mod stubs;

pub use commands::{
    dump, export, extract, keyspace_path, list_callers, list_keyspaces, query, violations,
};
pub use enrich::{enrich, EnrichVerb};
pub use error::CfdbCliError;
pub use scope::scope;
pub use stubs::{
    diff, drop_keyspace_cmd, list_items_matching, schema_describe_cmd, snapshots, typed_stub,
};

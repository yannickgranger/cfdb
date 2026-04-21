//! CLI command fixtures (Issue #126 v0.2-1 coverage gate).
//!
//! Two shapes:
//! - `#[derive(Parser)]` on a struct.
//! - `#[derive(Subcommand)]` on an enum.
//!
//! Stand-in traits match the `tests/entry_point.rs` pattern — the
//! scanner is textual on the attribute content, so a bare `Parser` /
//! `Subcommand` identifier fires regardless of its crate of origin.

// Stand-ins for the real `clap::Parser` / `clap::Subcommand` traits.
// The extractor matches on the attribute text "Parser" / "Subcommand",
// so any user-defined trait of the same name works.
pub trait Parser {}
pub trait Subcommand {}

/// First CLI command — `#[derive(Parser)]` top-level struct.
#[derive(Parser)]
pub struct RunCmd {
    pub workspace: String,
    pub db: String,
}

/// Second CLI command — `#[derive(Subcommand)]` enum. The scan fires
/// on the `Subcommand` token in the derive list.
#[derive(Subcommand)]
pub enum Verb {
    Extract,
    Query,
    Violations,
}

/// Control struct — no clap derive, must NOT be emitted.
pub struct UnrelatedConfig {
    pub flag: bool,
}

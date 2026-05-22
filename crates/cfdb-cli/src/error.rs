//! Typed error enum for `cfdb-cli` command handlers (#22).
//!
//! Replaces the previous `Result<_, Box<dyn std::error::Error>>` with a
//! richer type so callers and tests can branch on error kind without
//! downcasting. Each variant wraps an upstream error type verbatim where
//! context is self-explanatory; [`CfdbCliError::Usage`] is the escape
//! hatch for runtime-validation failures the CLI raises itself (unknown
//! flag values, missing keyspaces, unsupported formats, malformed
//! `--params` shapes).
//!
//! `From<String>` and `From<&str>` both route into `Usage` so the many
//! `Err("message".into())` and `Err(format!("...").into())` sites scattered
//! across the handlers keep working verbatim.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum CfdbCliError {
    /// Extractor failure walking the target workspace — bad Cargo.toml,
    /// unreadable `.rs` file, syn parse error, cargo metadata failure.
    ///
    /// Feature-gated on `lang-rust` because `cfdb-extractor` is now
    /// an optional dep (RFC-041 Phase 1 / Slice 41-C); slim builds
    /// (`--no-default-features`) drop the variant entirely. New code
    /// dispatches via the `LanguageProducer` trait and surfaces
    /// failures as [`CfdbCliError::Lang`] instead — `Extract` survives
    /// only for backward-compat with consumers that may still hold a
    /// `cfdb_extractor::ExtractError` directly (e.g. via the legacy
    /// `cfdb_extractor::extract_workspace` public shim).
    #[cfg(feature = "lang-rust")]
    #[error("extract failed: {0}")]
    Extract(#[from] cfdb_extractor::ExtractError),

    /// `LanguageProducer` failure surfaced through the dispatcher
    /// (RFC-041 §3.4). Every dispatch through `&dyn LanguageProducer`
    /// returns `cfdb_lang::LanguageError`, which `?`-propagates here
    /// — this is the variant new code paths land on.
    #[error("language producer failed: {0}")]
    Lang(#[from] cfdb_lang::LanguageError),

    /// `cfdb extract` was invoked but no compiled-in producer
    /// accepted the workspace. Carries the workspace path + the
    /// names of producers that WERE compiled in (so the user can
    /// diagnose: typical cause is a slim build without the right
    /// `lang-*` feature). Mapped via `#[from]` from
    /// [`crate::lang::NoProducerDetected`].
    #[error(transparent)]
    NoProducer(#[from] crate::lang::NoProducerDetected),

    /// Any `StoreBackend` method (ingest / execute / dump / enrich /
    /// drop_keyspace) OR `cfdb_petgraph::persist::{save, load}` — both
    /// surface through the same `StoreError` enum. Passed through
    /// transparently because `StoreError::Display` already renders a
    /// human-readable message.
    #[error(transparent)]
    Store(#[from] cfdb_core::store::StoreError),

    /// Cypher-subset parser failure on user-supplied input (`cfdb query`).
    /// Call sites that parse embedded templates (list-callers, hsb-by-name)
    /// add their own context by wrapping into [`CfdbCliError::Usage`].
    #[error("parse error: {0}")]
    Parse(#[from] cfdb_query::parser::ParseError),

    /// Filesystem I/O — reading a rule file, creating the `--db` output
    /// directory, writing a `--output` file, enumerating keyspaces.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON failure — `--params` deserialization or `to_string_pretty`
    /// serialization.
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    /// Runtime-validation failure — unknown keyspace, unsupported
    /// `--format`, malformed `--params` shape, missing `--db` directory,
    /// unknown bounded context, etc. The string IS the user-facing
    /// message (no prefix added by `Display`).
    #[error("{0}")]
    Usage(String),
}

impl From<String> for CfdbCliError {
    fn from(s: String) -> Self {
        CfdbCliError::Usage(s)
    }
}

impl From<&str> for CfdbCliError {
    fn from(s: &str) -> Self {
        CfdbCliError::Usage(s.to_string())
    }
}

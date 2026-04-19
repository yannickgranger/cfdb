//! Error type for HIR extraction — covers workspace-load failures,
//! VFS failures, and project-model failures propagated out to the
//! caller (typically `cfdb extract --hir`).

use std::path::PathBuf;

use thiserror::Error;

/// The single error type produced by `build_hir_database` and
/// `extract_call_sites`. Variants wrap lower-layer `ra-ap-*` errors
/// as `String` rather than the ra-ap-* concrete types so that the
/// `HirError` does NOT carry any `ra_ap_*` type in its public
/// signature (RFC-029 §A1.2 boundary contract).
#[derive(Debug, Error)]
pub enum HirError {
    /// Cargo workspace could not be discovered at the given root.
    #[error("workspace at {root}: {message}")]
    WorkspaceDiscovery { root: PathBuf, message: String },

    /// `ra_ap_load_cargo::load_workspace_at` returned an error.
    #[error("load_workspace_at({root}): {message}")]
    LoadWorkspace { root: PathBuf, message: String },

    /// Parsing a file returned a fatal syntax error that prevents
    /// HIR analysis from proceeding.
    #[error("parse failed for {file}: {message}")]
    Parse { file: PathBuf, message: String },
}

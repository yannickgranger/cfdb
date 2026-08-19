use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum HirError {
    #[error("workspace at {root}: {message}")]
    WorkspaceDiscovery { root: PathBuf, message: String },

    #[error("load_workspace_at({root}): {message}")]
    LoadWorkspace { root: PathBuf, message: String },

    #[error("parse failed for {file}: {message}")]
    Parse { file: PathBuf, message: String },
}

//! `build_hir_database` — load a Cargo workspace into a monomorphic
//! salsa `RootDatabase`, paired with the VFS needed to enumerate
//! files during extraction.
//!
//! RFC-029 §A1.2: the returned database is a CONCRETE `RootDatabase`
//! — not `dyn HirDatabase`. `ra_ap_hir::db::HirDatabase` is a salsa
//! query database with associated types and generic methods; it is
//! explicitly NOT object-safe. Every downstream function that accepts
//! the database accepts it monomorphically via `impl HirDatabase +
//! Sized` bounds, honoring the architect's object-safety constraint.

use std::path::{Path, PathBuf};

use ra_ap_ide_db::RootDatabase;
use ra_ap_load_cargo::{load_workspace_at, LoadCargoConfig, ProcMacroServerChoice};
use ra_ap_project_model::CargoConfig;
use ra_ap_vfs::Vfs;

use crate::error::HirError;

/// Load a Cargo workspace into a ready-to-query `(RootDatabase, Vfs)`
/// pair. The `Vfs` is required for file enumeration during extraction
/// (see [`crate::call_site_emitter::extract_call_sites`]).
///
/// # Errors
///
/// Returns [`HirError::WorkspaceDiscovery`] if the cargo workspace
/// cannot be located at `workspace_root`, or [`HirError::LoadWorkspace`]
/// if the workspace loads but the database materialization fails
/// (e.g., project-model error, proc-macro server failure).
///
/// # Determinism
///
/// `load_workspace_at` is deterministic for a given source tree —
/// running it twice on the same workspace produces structurally
/// identical databases. The VFS's `iter()` order is stable within a
/// single run; extraction code that iterates the VFS MUST sort its
/// output for G1 byte-stability across runs (see
/// [`crate::call_site_emitter`]).
pub fn build_hir_database(workspace_root: &Path) -> Result<(RootDatabase, Vfs), HirError> {
    let cargo_config = CargoConfig::default();
    let load_config = LoadCargoConfig {
        load_out_dirs_from_check: false,
        with_proc_macro_server: ProcMacroServerChoice::None,
        prefill_caches: false,
        num_worker_threads: 0,
        proc_macro_processes: 0,
    };

    let (db, vfs, _proc_macro_client) =
        load_workspace_at(workspace_root, &cargo_config, &load_config, &|_| {}).map_err(|e| {
            HirError::LoadWorkspace {
                root: PathBuf::from(workspace_root),
                message: e.to_string(),
            }
        })?;

    Ok((db, vfs))
}

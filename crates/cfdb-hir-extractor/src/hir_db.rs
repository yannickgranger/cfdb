//! `build_hir_database` — load a Cargo workspace into a monomorphic
//! salsa `RootDatabase`, paired with the VFS needed to enumerate
//! files during extraction.
//!
//! The returned database is a CONCRETE `RootDatabase` — not `dyn
//! HirDatabase`. `ra_ap_hir::db::HirDatabase` is a salsa query database
//! with associated types and generic methods; it is explicitly NOT
//! object-safe. Every downstream function that accepts the database
//! accepts it monomorphically via `impl HirDatabase + Sized` bounds.
//!
//! The function additionally returns an `Option<ProcMacroClient>` whose
//! `Some` arm owns the proc-macro subprocess handle. Salsa keeps live
//! references to expanders inside the DB; dropping the client while
//! salsa still holds them kills the subprocess and breaks any lazy
//! expansion during the caller's VFS walk. Callers MUST hold the
//! client alongside the DB for the duration of extraction.

use std::path::{Path, PathBuf};
use std::process::Command;

use ra_ap_ide_db::RootDatabase;
use ra_ap_load_cargo::{load_workspace, LoadCargoConfig, ProcMacroServerChoice};
use ra_ap_proc_macro_api::ProcMacroClient;
use ra_ap_project_model::{
    CargoConfig, CargoWorkspace, ProjectManifest, ProjectWorkspace, ProjectWorkspaceKind,
    RustLibSource, TargetKind,
};
use ra_ap_vfs::{AbsPathBuf, Vfs};

use cfdb_core::qname::TargetDiscriminator;

use crate::error::HirError;
use crate::target_map::TargetRootMap;

/// Load a Cargo workspace into a ready-to-query
/// `(RootDatabase, Vfs, Option<ProcMacroClient>, TargetRootMap)`
/// quadruple. The `Vfs` is required for file enumeration during
/// extraction (see [`crate::call_site_emitter::extract_call_sites`]).
/// The `Option<ProcMacroClient>` owns the proc-macro subprocess; it is
/// `Some` when proc-macro expansion is active and `None` otherwise.
/// Callers MUST hold the client alongside the database for the duration
/// of extraction. The [`TargetRootMap`] is the target-root correlation
/// map — callers pass it to the emitters so ids route through the
/// discriminated identity formulas.
///
/// `proc_macros` selects the loader policy:
/// - `true` (default in production) — enable `ProcMacroServerChoice::Sysroot`
///   with `proc_macro_processes = 1` when the sysroot binary is available.
///   When the binary is missing (typical CI-on-stock-rustc) the function
///   emits a stderr warning and silently falls back to `None + 0`.
/// - `false` — `ProcMacroServerChoice::None` with `proc_macro_processes = 0`.
///   No probe; no warning. The operator selects this via `cfdb extract
///   --no-proc-macro`.
///
/// # Errors
///
/// Returns [`HirError::WorkspaceDiscovery`] if the cargo workspace
/// cannot be located at `workspace_root`, or [`HirError::LoadWorkspace`]
/// if the workspace loads but the database materialization fails
/// (e.g., project-model error, proc-macro server failure AFTER the
/// availability probe passed).
///
/// # Determinism
///
/// `load_workspace_at` is deterministic for a given source tree —
/// running it twice on the same workspace produces structurally
/// identical databases. The VFS's `iter()` order is stable within a
/// single run; extraction code that iterates the VFS MUST sort its
/// output for G1 byte-stability across runs (see
/// [`crate::call_site_emitter`]).
pub fn build_hir_database(
    workspace_root: &Path,
    proc_macros: bool,
) -> Result<(RootDatabase, Vfs, Option<ProcMacroClient>, TargetRootMap), HirError> {
    let mut cargo_config = CargoConfig::default();
    if proc_macros {
        // `ws.find_sysroot_proc_macro_srv()` (called inside
        // `load_workspace`) returns None unless the workspace's
        // sysroot has been discovered. `RustLibSource::Discover` tells
        // ra_ap_project_model to auto-detect via `rustc --print sysroot`
        // at workspace load. Without this, requesting
        // `ProcMacroServerChoice::Sysroot` silently degrades — the
        // probe in `build_load_config` sees the binary on disk but
        // ra-analyzer never consults its location.
        cargo_config.sysroot = Some(RustLibSource::Discover);
    }
    let load_config = build_load_config(proc_macros);

    // Canonicalize the caller-spelled root before discovery (#561 /
    // #527 discipline via the single cfdb-lang resolver) so every
    // downstream path — cargo metadata target roots, VFS file paths —
    // is anchored on one canonical spelling and the emitters'
    // workspace-relative `file` props strip cleanly.
    let canonical_root = cfdb_lang::canonical_workspace_root(workspace_root).map_err(|e| {
        HirError::WorkspaceDiscovery {
            root: PathBuf::from(workspace_root),
            message: e.to_string(),
        }
    })?;

    // RFC-054 54-C: the one-shot `load_workspace_at` is split into its
    // two public steps so the `CargoWorkspace` (discarded by the
    // one-shot form) can be retained long enough to build the
    // target-root correlation map — ra_ap keeps no target kind on the
    // crate graph itself (`TargetData.kind` collapses into
    // `is_proc_macro`; see `crate::target_map` module docs).
    let manifest =
        ProjectManifest::discover_single(&AbsPathBuf::assert_utf8(canonical_root.clone()))
            .map_err(|e| HirError::WorkspaceDiscovery {
                root: canonical_root.clone(),
                message: e.to_string(),
            })?;
    let workspace = ProjectWorkspace::load(manifest, &cargo_config, &|_| {}).map_err(|e| {
        HirError::LoadWorkspace {
            root: canonical_root.clone(),
            message: e.to_string(),
        }
    })?;

    let targets = match &workspace.kind {
        ProjectWorkspaceKind::Cargo { cargo, .. } => build_target_root_map(cargo),
        // Non-cargo project layouts (rust-project.json, detached files)
        // have no cargo targets — every crate resolves to Lib.
        _ => TargetRootMap::default(),
    };

    let (db, vfs, proc_macro_client) =
        load_workspace(workspace, &cargo_config.extra_env, &load_config).map_err(|e| {
            HirError::LoadWorkspace {
                root: canonical_root.clone(),
                message: e.to_string(),
            }
        })?;

    Ok((db, vfs, proc_macro_client, targets))
}

/// Record every workspace member's `[lib]` / `[[bin]]` target root.
/// Test / example / bench / build-script targets and non-member
/// packages are deliberately absent — they resolve to
/// [`TargetDiscriminator::Lib`] via the map's miss policy (RFC-054
/// discriminates lib vs bin only; everything else keeps byte-stable
/// undiscriminated ids).
fn build_target_root_map(cargo: &CargoWorkspace) -> TargetRootMap {
    let mut entries: Vec<(PathBuf, TargetDiscriminator)> = Vec::new();
    for pkg in cargo.packages() {
        let pkg_data = &cargo[pkg];
        if !pkg_data.is_member {
            continue;
        }
        for &tgt in &pkg_data.targets {
            let target = &cargo[tgt];
            let discriminator = match target.kind {
                TargetKind::Bin => TargetDiscriminator::Bin {
                    name: target.name.clone(),
                },
                TargetKind::Lib { .. } => TargetDiscriminator::Lib,
                TargetKind::Example
                | TargetKind::Test
                | TargetKind::Bench
                | TargetKind::BuildScript
                | TargetKind::Other => continue,
            };
            let root = PathBuf::from(target.root.as_str());
            // Hardening (adversarial finding 2): ra_ap never resolves
            // symlinks (`AbsPathBuf::canonicalize` is a forbidden
            // stub), and only the TOP-LEVEL root is canonicalized
            // before discovery. A member behind an in-tree symlink can
            // make cargo-metadata's spelling and the VFS spelling of
            // one physical root diverge. Recording BOTH spellings (one
            // extra syscall per target) closes the map-side half; a
            // VFS spelling matching NEITHER remains a documented
            // residual that degrades that crate to Lib (see
            // target_map.rs module docs).
            if let Ok(canonical) = root.canonicalize() {
                if canonical != root {
                    entries.push((canonical, discriminator.clone()));
                }
            }
            entries.push((root, discriminator));
        }
    }
    TargetRootMap::from_entries(entries)
}

/// Construct the `LoadCargoConfig` for the requested policy. Production
/// callers reach this through the public `build_hir_database`; tests
/// dependency-inject a probe via [`build_load_config_with_probe`].
///
/// Factored out per RFC-043 R2 solid-architect CR1 so
/// `with_proc_macro_server` and `proc_macro_processes` share a single
/// source-of-truth boolean `pm_enabled` — the two fields CANNOT diverge
/// in the probe-fires-false path.
pub fn build_load_config(proc_macros: bool) -> LoadCargoConfig {
    build_load_config_with_probe(proc_macros, proc_macro_server_available)
}

/// Probe-injectable variant of [`build_load_config`]. Tests pass an
/// explicit probe closure to exercise the §3.3 case 1 path without
/// requiring a real sysroot.
pub fn build_load_config_with_probe(
    proc_macros: bool,
    probe: impl FnOnce() -> bool,
) -> LoadCargoConfig {
    let pm_enabled = proc_macros && probe();
    if proc_macros && !pm_enabled {
        // RFC-043 §3.3 case 1: requested but the sysroot binary is
        // missing. Silent API fallback, loud stderr warning so the
        // operator can install the missing component or pass
        // `--no-proc-macro` to silence the warning.
        eprintln!(
            "cfdb-hir-extractor: proc-macro server requested but \
             `rust-analyzer-proc-macro-srv` is not present in the active \
             sysroot. Falling back to syn-only resolution; receiver-type \
             recall on macro-touched code is reduced. Install via \
             `rustup component add rust-analyzer` (or pass --no-proc-macro \
             to silence this warning)."
        );
    }
    LoadCargoConfig {
        load_out_dirs_from_check: false,
        with_proc_macro_server: if pm_enabled {
            ProcMacroServerChoice::Sysroot
        } else {
            ProcMacroServerChoice::None
        },
        prefill_caches: false,
        num_worker_threads: 0,
        proc_macro_processes: if pm_enabled { 1 } else { 0 },
    }
}

/// Probe whether `rust-analyzer-proc-macro-srv` is available in the
/// active sysroot. Returns `false` when the binary is absent (typical
/// stock-rustc CI image) and `true` otherwise.
///
/// The probe checks `$(rustc --print sysroot)/libexec/rust-analyzer-proc-macro-srv`,
/// which is the canonical rustup layout and matches the path that
/// `ra_ap_project_model::ProjectWorkspace::find_sysroot_proc_macro_srv`
/// itself consults.
pub fn proc_macro_server_available() -> bool {
    proc_macro_server_path().is_some()
}

fn proc_macro_server_path() -> Option<PathBuf> {
    let output = Command::new("rustc")
        .arg("--print")
        .arg("sysroot")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let sysroot = std::str::from_utf8(&output.stdout).ok()?.trim();
    let candidate = PathBuf::from(sysroot)
        .join("libexec")
        .join("rust-analyzer-proc-macro-srv");
    candidate.exists().then_some(candidate)
}

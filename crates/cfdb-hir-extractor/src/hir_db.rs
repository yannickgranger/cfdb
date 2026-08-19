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

pub fn build_hir_database(
    workspace_root: &Path,
    proc_macros: bool,
) -> Result<(RootDatabase, Vfs, Option<ProcMacroClient>, TargetRootMap), HirError> {
    let mut cargo_config = CargoConfig::default();
    if proc_macros {
        cargo_config.sysroot = Some(RustLibSource::Discover);
    }
    let load_config = build_load_config(proc_macros);

    let canonical_root = cfdb_lang::canonical_workspace_root(workspace_root).map_err(|e| {
        HirError::WorkspaceDiscovery {
            root: PathBuf::from(workspace_root),
            message: e.to_string(),
        }
    })?;

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

pub fn build_load_config(proc_macros: bool) -> LoadCargoConfig {
    build_load_config_with_probe(proc_macros, proc_macro_server_available)
}

pub fn build_load_config_with_probe(
    proc_macros: bool,
    probe: impl FnOnce() -> bool,
) -> LoadCargoConfig {
    let pm_enabled = proc_macros && probe();
    if proc_macros && !pm_enabled {
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

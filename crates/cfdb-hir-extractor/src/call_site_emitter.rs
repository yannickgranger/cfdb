use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use cfdb_core::fact::{Edge, Node};
use ra_ap_hir::db::HirDatabase;
use ra_ap_hir::Semantics;
use ra_ap_hir_ty::attach_db;
use ra_ap_ide_db::line_index::LineIndex;
use ra_ap_vfs::{Vfs, VfsPath};

use crate::error::HirError;
use crate::target_map::{EmitCtx, TargetRootMap};

mod facts;
mod naming;
mod walk;

pub(crate) use naming::function_qname;
use walk::walk_file;

pub fn extract_call_sites<DB>(
    db: &DB,
    vfs: &Vfs,
    workspace_root: &Path,
    targets: &TargetRootMap,
) -> Result<(Vec<Node>, Vec<Edge>), HirError>
where
    DB: HirDatabase + Sized,
{
    attach_db(db, || {
        extract_call_sites_attached(db, vfs, workspace_root, targets)
    })
}

fn extract_call_sites_attached<DB>(
    db: &DB,
    vfs: &Vfs,
    workspace_root: &Path,
    targets: &TargetRootMap,
) -> Result<(Vec<Node>, Vec<Edge>), HirError>
where
    DB: HirDatabase + Sized,
{
    let sema = Semantics::new(db);
    let ctx = EmitCtx { vfs, targets };
    let mut nodes: Vec<Node> = Vec::new();
    let mut edges: Vec<Edge> = Vec::new();

    let files = workspace_rs_files(vfs, workspace_root)?;

    for (file_id, file_path) in files {
        let source_file = sema.parse_guess_edition(file_id);
        let file_text_handle = db.file_text(file_id);
        let file_text: &str = file_text_handle.text(db);
        let line_index = LineIndex::new(file_text);
        let mut counts: BTreeMap<(String, String), usize> = BTreeMap::new();
        walk_file(
            &sema,
            &ctx,
            &source_file,
            &file_path,
            &line_index,
            &mut counts,
            &mut nodes,
            &mut edges,
        );
    }

    nodes.sort_by(|a, b| a.id.cmp(&b.id));
    edges.sort_by(|a, b| {
        (a.src.as_str(), a.dst.as_str(), a.label.as_str()).cmp(&(
            b.src.as_str(),
            b.dst.as_str(),
            b.label.as_str(),
        ))
    });

    Ok((nodes, edges))
}

fn vfs_path_to_pathbuf(p: &VfsPath) -> Option<PathBuf> {
    p.as_path().map(|abs| PathBuf::from(abs.as_str()))
}

pub(crate) fn workspace_rs_files(
    vfs: &Vfs,
    workspace_root: &Path,
) -> Result<Vec<(ra_ap_vfs::FileId, PathBuf)>, HirError> {
    let canonical_root = cfdb_lang::canonical_workspace_root(workspace_root).map_err(|e| {
        HirError::WorkspaceDiscovery {
            root: PathBuf::from(workspace_root),
            message: e.to_string(),
        }
    })?;
    let mut files: Vec<(ra_ap_vfs::FileId, PathBuf)> = vfs
        .iter()
        .filter_map(|(file_id, vfs_path)| {
            let p = vfs_path_to_pathbuf(vfs_path)?;
            if p.extension().and_then(|e| e.to_str()) != Some("rs") {
                return None;
            }
            let rel = p.strip_prefix(&canonical_root).ok()?;
            Some((file_id, rel.to_path_buf()))
        })
        .collect();
    files.sort_by(|a, b| a.1.cmp(&b.1));
    Ok(files)
}

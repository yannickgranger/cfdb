use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use cfdb_core::fact::{Edge, Node, PropValue};
use cfdb_core::qname::module_qpath;
use cfdb_core::schema::{EdgeLabel, Label};
use syn::visit::Visit;

use crate::item_visitor::ItemVisitor;
use crate::{Emitter, ExtractError};

pub(crate) struct PendingExternalMod {
    pub(crate) name: String,
    pub(crate) path_override: Option<String>,
    pub(crate) is_test: bool,
}

pub(crate) fn visit_file(
    emitter: &mut Emitter,
    crate_id: &str,
    crate_name: &str,
    bounded_context: &str,
    target: &cfdb_core::qname::TargetDiscriminator,
    file_path: &Path,
    workspace_root: &Path,
) -> Result<(), ExtractError> {
    visit_file_inner(
        emitter,
        crate_id,
        crate_name,
        bounded_context,
        target,
        file_path,
        workspace_root,
        vec![crate_name.replace('-', "_")],
        false,
    )
}

#[allow(clippy::too_many_arguments)]
fn visit_file_inner(
    emitter: &mut Emitter,
    crate_id: &str,
    crate_name: &str,
    bounded_context: &str,
    target: &cfdb_core::qname::TargetDiscriminator,
    file_path: &Path,
    workspace_root: &Path,
    module_stack: Vec<String>,
    inherited_test: bool,
) -> Result<(), ExtractError> {
    let source = std::fs::read_to_string(file_path).map_err(|e| ExtractError::Io {
        path: file_path.to_path_buf(),
        source: e,
    })?;
    let ast = syn::parse_file(&source).map_err(|e| ExtractError::Parse {
        path: file_path.to_path_buf(),
        message: e.to_string(),
    })?;

    let rel_path = file_path
        .strip_prefix(workspace_root)
        .map_err(|_| ExtractError::PathNotInWorkspace {
            file: file_path.to_path_buf(),
            workspace_root: workspace_root.to_path_buf(),
        })?
        .to_string_lossy()
        .into_owned();

    let file_id = format!("file:{crate_name}:{rel_path}");
    emitter.emit_node(Node {
        id: file_id.clone(),
        label: Label::new(Label::FILE),
        props: {
            let mut p = BTreeMap::new();
            p.insert("path".into(), PropValue::Str(rel_path.clone()));
            p.insert("crate".into(), PropValue::Str(crate_name.to_string()));
            p.insert("is_test".into(), PropValue::Bool(inherited_test));
            p
        },
    });
    if module_stack.len() > 1 {
        let qpath = module_qpath(&module_stack);
        let module_id = format!("module:{qpath}");
        emitter.emit_edge(Edge {
            src: file_id.clone(),
            dst: module_id,
            label: EdgeLabel::new(EdgeLabel::IN_MODULE),
            props: BTreeMap::new(),
        });
    }

    let mut visitor = ItemVisitor {
        emitter,
        crate_id: crate_id.to_string(),
        crate_name: crate_name.to_string(),
        file_path: rel_path,
        bounded_context: bounded_context.to_string(),
        target: target.clone(),
        module_stack: module_stack.clone(),
        pending_external_mods: Vec::new(),
        current_impl_target: None,
        test_mod_depth: if inherited_test { 1 } else { 0 },
    };
    visitor.visit_file(&ast);
    let pending = std::mem::take(&mut visitor.pending_external_mods);

    for pending_mod in pending {
        descend_into_pending_mod(
            emitter,
            crate_id,
            crate_name,
            bounded_context,
            target,
            file_path,
            workspace_root,
            &module_stack,
            inherited_test,
            pending_mod,
        )?;
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn descend_into_pending_mod(
    emitter: &mut Emitter,
    crate_id: &str,
    crate_name: &str,
    bounded_context: &str,
    target: &cfdb_core::qname::TargetDiscriminator,
    file_path: &Path,
    workspace_root: &Path,
    module_stack: &[String],
    inherited_test: bool,
    pending_mod: PendingExternalMod,
) -> Result<(), ExtractError> {
    let Some(child_path) = resolve_external_module(
        file_path,
        &pending_mod.name,
        pending_mod.path_override.as_deref(),
    ) else {
        return Ok(());
    };
    let mut child_stack = module_stack.to_vec();
    child_stack.push(pending_mod.name);
    visit_file_inner(
        emitter,
        crate_id,
        crate_name,
        bounded_context,
        target,
        &child_path,
        workspace_root,
        child_stack,
        inherited_test || pending_mod.is_test,
    )
}

fn resolve_external_module(
    current: &Path,
    mod_name: &str,
    path_override: Option<&str>,
) -> Option<PathBuf> {
    let file_stem = current.file_stem()?.to_str()?;
    let parent = current.parent()?;

    if let Some(p) = path_override {
        let base = if matches!(file_stem, "lib" | "main" | "mod") {
            parent.to_path_buf()
        } else {
            parent.join(file_stem)
        };
        let mut candidates = vec![base.join(p), parent.join(p)];
        candidates.retain(|c| !c.as_os_str().is_empty());
        return candidates.into_iter().find(|p| p.exists());
    }

    let candidates: Vec<PathBuf> = if matches!(file_stem, "lib" | "main" | "mod") {
        vec![
            parent.join(format!("{mod_name}.rs")),
            parent.join(mod_name).join("mod.rs"),
        ]
    } else {
        let sibling_dir = parent.join(file_stem);
        vec![
            sibling_dir.join(format!("{mod_name}.rs")),
            sibling_dir.join(mod_name).join("mod.rs"),
        ]
    };

    candidates.into_iter().find(|p| p.exists())
}

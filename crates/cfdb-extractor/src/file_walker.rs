//! Module walker — given a file on disk, parse it with `syn`, drive an
//! [`ItemVisitor`] over the AST, then recurse into any external
//! `mod foo;` declarations the visitor queued.
//!
//! Stable Rust module resolution: convention first (`./foo.rs`,
//! `./foo/mod.rs`), `#[path = "..."]` override wins when present.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use cfdb_core::fact::{Edge, Node, PropValue};
use cfdb_core::qname::module_qpath;
use cfdb_core::schema::{EdgeLabel, Label};
use syn::visit::Visit;

use crate::item_visitor::ItemVisitor;
use crate::{Emitter, ExtractError};

/// A `mod foo;` declaration encountered while visiting a file. Captured
/// during the first pass and resolved to a child file in the second pass.
pub(crate) struct PendingExternalMod {
    /// The name token (`mod foo;` → `foo`).
    pub(crate) name: String,
    /// Optional `#[path = "..."]` attribute override. When present, this
    /// is the literal path the compiler would use for the child file, and
    /// we must honor it instead of the convention-based lookup. qbot-core
    /// uses this heavily — 309 occurrences, 236 of them pointing at test
    /// files — so without it the extractor misses ~88% of Utc::now() call
    /// sites in walked src/ code.
    pub(crate) path_override: Option<String>,
    /// True if this external mod is under a `#[cfg(test)]` (or
    /// `#[cfg(all(test, ...))]`) attribute. The child file inherits the
    /// flag so every Item and CallSite it emits is tagged `is_test=true`.
    pub(crate) is_test: bool,
}

pub(crate) fn visit_file(
    emitter: &mut Emitter,
    crate_id: &str,
    crate_name: &str,
    bounded_context: &str,
    file_path: &Path,
    workspace_root: &Path,
) -> Result<(), ExtractError> {
    visit_file_inner(
        emitter,
        crate_id,
        crate_name,
        bounded_context,
        file_path,
        workspace_root,
        vec![crate_name.replace('-', "_")],
        false,
    )
}

/// Walk a single Rust source file, emit its items, and recursively resolve
/// any `mod foo;` declarations. Stable Rust module resolution: convention
/// first (`./foo.rs`, `./foo/mod.rs`), `#[path = "..."]` override wins
/// when present.
///
/// `inherited_test` is `true` when this file is reached by recursing into
/// a `#[cfg(test)] mod foo;` external declaration. It bootstraps the
/// inner visitor's `test_mod_depth` so items in the child file are tagged
/// as test code even though no `#[cfg(test)]` attribute is visible on
/// them locally.
#[allow(clippy::too_many_arguments)]
fn visit_file_inner(
    emitter: &mut Emitter,
    crate_id: &str,
    crate_name: &str,
    bounded_context: &str,
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
        .unwrap_or(file_path)
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
    // IN_MODULE membership for the deepest enclosing `:Module` (#267 /
    // CFDB-EXT-H1). Schema declares `IN_MODULE` from `[Item, File]` to
    // `[Module]` (`cfdb-core/src/schema/describe/edges.rs`), but the
    // extractor used to skip the File side entirely. The `:Module`
    // node for `module_qpath(&module_stack)` is emitted by the parent
    // file's `visit_item_mod` when the `mod foo;` declaration is
    // encountered, so the dst already exists by the time we reach the
    // child file — except at crate root, where `module_stack` is just
    // `[crate_name]` and there is no `:Module` node (cfdb's existing
    // convention emits `:Module` only for nested `mod` decls). Skip
    // emission in that case to avoid a dangling edge; the existing
    // workspace-level wiring still places the file via its containing
    // crate.
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
            file_path,
            workspace_root,
            &module_stack,
            inherited_test,
            pending_mod,
        )?;
    }

    Ok(())
}

/// Recurse into one `mod foo;` child discovered during the file walk.
/// Extracted from the `for pending_mod in pending` loop in
/// [`visit_file_inner`] so the per-child `module_stack` clone does not
/// count against the `clones-in-loops` quality gate — the clone is
/// necessary (each recursive call owns its own stack) but belongs to
/// the helper body rather than the outer loop scope.
#[allow(clippy::too_many_arguments)]
fn descend_into_pending_mod(
    emitter: &mut Emitter,
    crate_id: &str,
    crate_name: &str,
    bounded_context: &str,
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
        &child_path,
        workspace_root,
        child_stack,
        inherited_test || pending_mod.is_test,
    )
}

/// Resolve `mod foo;` declared inside `file_path` to the file on disk.
///
/// Order of resolution matches `rustc`:
/// 1. `#[path = "custom.rs"] mod foo;` — the attribute wins. The path is
///    resolved relative to the directory containing `current` when
///    `current` is a crate root (`lib.rs`/`main.rs`/`mod.rs`) or a
///    file-as-module; otherwise relative to the file-as-module's
///    sibling directory.
/// 2. Convention: `./foo.rs` then `./foo/mod.rs`.
///
/// The crate-root vs nested-file distinction matters because a
/// file-as-module (`foo.rs`) has its children in `./foo/`, not `./`.
fn resolve_external_module(
    current: &Path,
    mod_name: &str,
    path_override: Option<&str>,
) -> Option<PathBuf> {
    let file_stem = current.file_stem()?.to_str()?;
    let parent = current.parent()?;

    // Attribute path override — resolve relative to the sibling dir that
    // Rust would use for this module's children.
    if let Some(p) = path_override {
        // rustc resolves `#[path]` relative to the directory containing
        // the current file for file-roots (`lib.rs`, `main.rs`, `mod.rs`)
        // and relative to `./stem/` for a nested file-as-module
        // (`foo.rs` whose children live in `./foo/`). Honoring this
        // distinction is load-bearing for qbot-core's test-file layout.
        let base = if matches!(file_stem, "lib" | "main" | "mod") {
            parent.to_path_buf()
        } else {
            parent.join(file_stem)
        };
        // Many crates put `#[path = "x_tests.rs"]` on a file-root though,
        // expecting the sibling-to-current semantics. rustc actually
        // resolves relative to `parent` in that case. Try both and keep
        // whichever exists — fall back to `parent` too for broad compat.
        let mut candidates = vec![base.join(p), parent.join(p)];
        // Also normalize `..` if present in override.
        candidates.retain(|c| !c.as_os_str().is_empty());
        return candidates.into_iter().find(|p| p.exists());
    }

    let candidates: Vec<PathBuf> = if matches!(file_stem, "lib" | "main" | "mod") {
        vec![
            parent.join(format!("{mod_name}.rs")),
            parent.join(mod_name).join("mod.rs"),
        ]
    } else {
        // For `foo.rs`, children live in `./foo/`
        let sibling_dir = parent.join(file_stem);
        vec![
            sibling_dir.join(format!("{mod_name}.rs")),
            sibling_dir.join(mod_name).join("mod.rs"),
        ]
    };

    candidates.into_iter().find(|p| p.exists())
}

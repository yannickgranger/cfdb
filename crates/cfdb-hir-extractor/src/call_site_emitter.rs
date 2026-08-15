//! Walk every source file in the VFS, resolve method-call and path-call
//! dispatch via `ra_ap_hir::Semantics`, and emit `:CallSite`,
//! `CALLS(item→item)`, and `INVOKES_AT(item→:CallSite)` facts into
//! cfdb-core's graph vocabulary.
//!
//! ## Resolved call shapes
//!
//! Two AST shapes produce resolved call facts:
//!
//! - **`ast::MethodCallExpr`** — `receiver.method(args)`. Resolved
//!   via `Semantics::resolve_method_call`. Wire `kind = "method"`.
//! - **`ast::CallExpr`** with a `PathExpr` function expression —
//!   plain function calls (`my_helper(args)`), associated-function
//!   calls (`MyType::new(args)`, `Foo::bar(args)`), and trait-static
//!   dispatch (`Trait::method(args)`) when statically resolvable
//!   to a concrete `hir::Function`. Resolved via
//!   `Semantics::resolve_path`. Wire `kind = "fn"` (the AST doesn't
//!   distinguish associated-function from free-fn at this site;
//!   downstream consumers can recheck via the resolved callee qname
//!   if they need the distinction). Issue #387 closed #85c's
//!   deferred follow-up scope.
//!
//! ## Out-of-scope dispatches
//!
//! Trait-object method calls (`(t: &dyn Trait).method()`) without
//! a static dispatch target, closure / function-pointer indirection,
//! and macro-expanded calls that ra-ap can't see through are still
//! deferred — they produce no resolved fact. Recall on these shapes
//! requires either type-inference precision the HIR doesn't currently
//! commit to, or a syn-side unresolved-CallSite parallel.
//!
//! ## Cross-extractor ID stability
//!
//! Every `item:<qname>` ID is derived via `cfdb_core::qname` (the
//! canonical formula shipped in #90). Both the syn-based
//! `cfdb-extractor` and this HIR-based extractor produce bit-identical
//! qnames for the same source item — without that, `CALLS(item:A,
//! item:B)` edges from this extractor would silently dangle against
//! `:Item` nodes emitted by the syn extractor.

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

/// Extract resolved call-site facts from a loaded HIR database.
///
/// Iterates every `.rs` file in `vfs`, parses each via
/// `Semantics::parse_guess_edition`, walks the syntax tree for method
/// calls, and resolves each via `Semantics::resolve_method_call`. Every
/// successful resolution emits one `:CallSite` node, one `CALLS` edge
/// from caller to callee, and one `INVOKES_AT` edge from caller to the
/// call site.
///
/// # Errors
///
/// Returns [`HirError`] on I/O or parsing failures. Individual method
/// calls that fail to resolve are silently skipped — an unresolved
/// call is not an error; it is simply data the HIR extractor cannot
/// claim resolution over. Note: this does NOT imply the syn extractor
/// has already seen the same call (syn may miss calls inside
/// macro-generated bodies that HIR can see but not resolve). The
/// semantic is "claim resolution only when precise" — HIR's
/// higher-precision / lower-recall tradeoff on generics and dynamic
/// dispatch is deliberate.
///
/// # Determinism
///
/// Output nodes and edges are sorted by ID before return, so two
/// invocations on the same workspace produce byte-identical vecs
/// regardless of the VFS iteration order chosen by `ra_ap_vfs`.
///
/// # Walk scope + `file` props
///
/// Only files under `workspace_root` are walked; dependency and sysroot
/// sources in the VFS are skipped. Dep-internal edges never join the
/// workspace graph. Scoping the walk also makes the workspace-relative
/// `file`-prop contract total: every emitted path strips against the
/// canonical root by construction, with no silent absolute fallback
/// possible.
pub fn extract_call_sites<DB>(
    db: &DB,
    vfs: &Vfs,
    workspace_root: &Path,
    targets: &TargetRootMap,
) -> Result<(Vec<Node>, Vec<Edge>), HirError>
where
    DB: HirDatabase + Sized,
{
    // hir-ty's next-solver reads from its own thread-local slot (separate
    // from salsa's). Without attaching it, HIR queries panic. The closure
    // returns owned Vecs so the attach scope ends before we return.
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
        // Build a LineIndex once per file so byte-offset → 1-indexed
        // source-line conversion is O(log n) per call site. The text
        // comes from salsa-cached `SourceDatabase::file_text`, so this
        // read is free; constructing the LineIndex is a single newline
        // scan amortised across every method-call we emit.
        let file_text_handle = db.file_text(file_id);
        let file_text: &str = file_text_handle.text(db);
        let line_index = LineIndex::new(file_text);
        // Per-call-site deduplication counter keyed by
        // `(caller_identity, callee_path)`. NOTE the known limitation:
        // each FileId is walked once and `sema.to_def` resolves a
        // syntax node to ONE crate — ra_ap's source_to_def documents
        // "no injective mapping ... return the first answer that
        // works" — so a file shared across two targets (`#[path]`)
        // attributes its calls to a SINGLE target; the other target's
        // copies are not captured (pre-existing HIR recall gap, unlike
        // the syn side which walks per target). Identity-keying the
        // counter is still correct for the target that wins.
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

    // Stable sort: nodes by id, edges by (src, dst, label).
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

/// Convert a `VfsPath` to a concrete filesystem path. In-memory
/// VFS paths (e.g. macro-expanded virtual files) return None.
fn vfs_path_to_pathbuf(p: &VfsPath) -> Option<PathBuf> {
    p.as_path().map(|abs| PathBuf::from(abs.as_str()))
}

/// Enumerate the workspace's own `.rs` files from the VFS, sorted by
/// path for deterministic order, each paired with its workspace-relative
/// path. Files outside the canonical root (dependency and sysroot sources)
/// are skipped. Shared with [`crate::entry_point_emitter`].
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
            // Workspace scope + relative form in one step. The strip
            // cannot silently mis-emit: a path that does not live under
            // the canonical root is not walked at all.
            let rel = p.strip_prefix(&canonical_root).ok()?;
            Some((file_id, rel.to_path_buf()))
        })
        .collect();
    files.sort_by(|a, b| a.1.cmp(&b.1));
    Ok(files)
}

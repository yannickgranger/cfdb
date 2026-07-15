//! `extract_call_sites` — walk every source file in the VFS,
//! resolve method-call AND path-call dispatch via `ra_ap_hir::Semantics`,
//! emit `:CallSite` + `CALLS(item→item)` + `INVOKES_AT(item→:CallSite)`
//! facts into cfdb-core's graph vocabulary.
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
use std::path::PathBuf;

use cfdb_core::fact::{Edge, Node};
use ra_ap_hir::db::HirDatabase;
use ra_ap_hir::Semantics;
use ra_ap_hir_ty::attach_db;
use ra_ap_ide_db::line_index::LineIndex;
use ra_ap_vfs::{Vfs, VfsPath};

use crate::error::HirError;

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
/// successful resolution emits exactly one `:CallSite` node (with
/// `resolver="hir"` + `callee_resolved=true`), one `CALLS(item:caller
/// → item:callee)` edge, and one `INVOKES_AT(item:caller →
/// :CallSite)` edge.
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
pub fn extract_call_sites<DB>(db: &DB, vfs: &Vfs) -> Result<(Vec<Node>, Vec<Edge>), HirError>
where
    DB: HirDatabase + Sized,
{
    // hir-ty's next-solver reads the database from its OWN thread-local
    // (separate from salsa's top-level attached slot). Without this
    // attach, any HIR query that dispatches through the solver panics
    // "Try to use attached db, but not db is attached". The closure
    // returns owned Vecs so the attach scope ends before we return.
    attach_db(db, || extract_call_sites_attached(db, vfs))
}

fn extract_call_sites_attached<DB>(db: &DB, vfs: &Vfs) -> Result<(Vec<Node>, Vec<Edge>), HirError>
where
    DB: HirDatabase + Sized,
{
    let sema = Semantics::new(db);
    let mut nodes: Vec<Node> = Vec::new();
    let mut edges: Vec<Edge> = Vec::new();

    // Collect files and sort by path for deterministic traversal.
    // The VFS iteration order is an implementation detail of salsa's
    // hash-set internals; sorting by path restores G1 byte-stability.
    let mut files: Vec<(ra_ap_vfs::FileId, PathBuf)> = vfs
        .iter()
        .filter_map(|(file_id, vfs_path)| {
            let p = vfs_path_to_pathbuf(vfs_path)?;
            if p.extension().and_then(|e| e.to_str()) == Some("rs") {
                Some((file_id, p))
            } else {
                None
            }
        })
        .collect();
    files.sort_by(|a, b| a.1.cmp(&b.1));

    for (file_id, file_path) in files {
        let source_file = sema.parse_guess_edition(file_id);
        // Build a LineIndex once per file so byte-offset → 1-indexed
        // source-line conversion in `walk_file` is O(log n) per call
        // site. The text comes from salsa-cached `SourceDatabase::file_text`,
        // so this read is free; constructing the LineIndex is a single
        // newline scan amortised across every method-call we emit. F-005
        // / EPIC #273: `:CallSite.line` in the HIR extractor was hardcoded
        // to 0 — this is the parity fix matching what PR #291 did for the
        // syn extractor.
        let file_text_handle = db.file_text(file_id);
        let file_text: &str = file_text_handle.text(db);
        let line_index = LineIndex::new(file_text);
        // Per-call-site deduplication counter keyed by
        // `(caller_qname, callee_path)`.
        let mut counts: BTreeMap<(String, String), usize> = BTreeMap::new();
        walk_file(
            &sema,
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

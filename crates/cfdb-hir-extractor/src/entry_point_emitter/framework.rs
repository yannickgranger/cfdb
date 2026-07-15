//! The `FrameworkDetector` registry seam (RFC-049 §3.1 / §3.2, slice
//! 49-0). The pre-existing HIR entry-point detection was a single
//! `match`-on-`SyntaxKind` dispatch inlined into the file scan; this
//! module lifts each framework's recogniser behind a registered
//! [`FrameworkDetector`] so that *adding* a framework becomes a
//! registration, not a new `match` arm (the OCP extension point the
//! RFC calls for).
//!
//! **Recall-neutral (49-0 invariant).** This slice is a behaviour-
//! preserving refactor: every detector reuses the parent module's
//! existing recogniser helpers verbatim, and [`extract_entry_points`]'s
//! final sort makes the per-detector pass order irrelevant to the
//! emitted bytes. cfdb's own `:EntryPoint` / `EXPOSES` /
//! `REGISTERS_PARAM` set is identical before and after.
//!
//! [`extract_entry_points`]: super::extract_entry_points
//!
//! **Manifest gating (RFC-049 §3.1).** Each framework detector is gated
//! on its framework's presence in the workspace crate graph
//! (`present(manifest)`), so a detector never runs on a workspace that
//! does not depend on its framework — the §4 "no false positives
//! off-framework" invariant. The [`Manifest`] is populated from the
//! loaded crate graph ([`Manifest::from_crate_graph`]) and carries the
//! workspace members' own direct dependency names. Slice 49-A (#494)
//! gates the clap
//! (`cli_command`) detector on the `clap` dependency; slice 49-B (#495)
//! gates the axum/actix (`http_route`) detector on `axum`/`actix_web`.
//! The MCP, cron and websocket detectors remain unconditionally present
//! — narrowing them is not in the 49-A/B scope.
//!
//! **Recall-neutral on cfdb-self.** cfdb depends on `clap` (the clap
//! detector fires identically before and after the gate) and on neither
//! axum nor actix-web (the http_route detector is inert, and cfdb's own
//! sources carry no route idiom, so its `:EntryPoint` set is unchanged).
//! A recall fixture that declares an idiom carries a stub dependency of
//! the framework's crate name so the gate observes it; a fixture with
//! the idiom but no framework dependency proves the detector inert (§4).
//!
//! **Language scoping (RFC-049 §3.1 Q4).** This registry is the *Rust*
//! detector set; its `detect` is parameterised by the Rust HIR AST. The
//! PHP / TypeScript detectors (49-C / 49-D) live in their own extractor
//! crates over their own (tree-sitter) ASTs and never share this trait
//! — a detector never reaches across a language-extractor boundary.

use std::collections::BTreeSet;
use std::path::Path;

use cfdb_core::fact::{Edge, Node};
use ra_ap_hir::db::HirDatabase;
use ra_ap_hir::{Crate, Semantics};
use ra_ap_syntax::ast::{self, AstNode};
use ra_ap_syntax::SyntaxKind;
use ra_ap_vfs::Vfs;

use super::registers_param::{
    emit_clap_enum_registers_param, emit_clap_struct_registers_param, emit_mcp_registers_param,
    has_clap_derive, has_tool_attr,
};

/// The workspace-member `[dependencies]` view a
/// [`FrameworkDetector::present`] gate consults (RFC-049 §3.1 — the
/// framework's crate in a workspace member's own dependency list).
///
/// Populated from the loaded crate graph by [`from_crate_graph`]: the
/// display-names of the direct dependencies of the workspace's own
/// member crates. A framework reachable only transitively (a dependency
/// of a non-member) is deliberately excluded, so a gate reports `false`
/// unless a member actually depends on the framework — the RFC-049 §4
/// "no false positives off-framework" invariant.
///
/// [`from_crate_graph`]: Manifest::from_crate_graph
pub(crate) struct Manifest {
    /// Normalized (`-` → `_`) display-names of the direct dependencies of
    /// the workspace's member crates.
    /// [`depends_on`](Manifest::depends_on) queries this set.
    dependency_names: BTreeSet<String>,
}

impl Manifest {
    /// Collect the direct `[dependencies]` of the workspace's own member
    /// crates from the loaded crate graph, scoped by `workspace_root`.
    /// For each crate, membership is decided by PATH CONTAINMENT: the
    /// crate's root-file path (via `vfs`) must lie under `workspace_root`.
    /// The display-name of each member's direct dependency is recorded.
    ///
    /// Path containment, not `CrateOrigin`, is the membership signal:
    /// ra_ap 0.0.328's `origin(db).is_local()` is set by project_model
    /// from `source.is_none()` (cargo_workspace.rs:399), which — per its
    /// own comment — "includes all members of the current workspace, AS
    /// WELL AS ANY PATH DEPENDENCY OUTSIDE THE WORKSPACE". So `is_local()`
    /// alone would let a sibling path-dependency's transitive framework
    /// dep leak in (defeating §4). The true `is_member` bit lives in
    /// project_model's `PackageData`, which is unreachable through
    /// `HirDatabase` — hence the root-file-path gate. `is_local()` is kept
    /// as a cheap pre-filter (it excludes sysroot/registry crates).
    ///
    /// Transitive dependencies of non-member crates are excluded on
    /// purpose: the gate consults a member's OWN manifest, so a framework
    /// pulled in only below some unrelated non-member never makes it
    /// "present" (RFC-049 §4). `CrateDisplayName` already renders the
    /// `-` → `_` normalized crate name, so `actix-web` compares equal to
    /// an `actix_web` gate key with no extra normalization here.
    pub(crate) fn from_crate_graph<DB: HirDatabase>(
        db: &DB,
        vfs: &Vfs,
        workspace_root: &Path,
    ) -> Self {
        // Canonicalize so the root matches the (canonical) crate-graph
        // file paths cargo metadata produces; fall back to the raw path.
        let ws_root = workspace_root
            .canonicalize()
            .unwrap_or_else(|_| workspace_root.to_path_buf());
        let mut dependency_names = BTreeSet::new();
        for krate in Crate::all(db) {
            if !krate.origin(db).is_local() {
                continue;
            }
            let Some(root_path) = super::vfs_path_to_pathbuf(vfs.file_path(krate.root_file(db)))
            else {
                continue;
            };
            // Canonicalize the candidate too: the vfs may carry a
            // different (symlinked/bind-mounted) alias of the same tree
            // than the canonicalized root, and a one-sided comparison
            // silently drops every member (observed on a dual-mounted
            // workspace). Fall back to the raw path when the file is
            // gone by containment-check time.
            let root_path = root_path.canonicalize().unwrap_or(root_path);
            if !root_path.starts_with(&ws_root) {
                continue;
            }
            for dep in krate.dependencies(db) {
                if let Some(display) = dep.krate.display_name(db) {
                    dependency_names.insert(display.to_string());
                }
            }
        }
        Self { dependency_names }
    }

    /// Whether a workspace member depends on the crate named
    /// `framework_crate` (given already normalized `-` → `_`).
    fn depends_on(&self, framework_crate: &str) -> bool {
        self.dependency_names.contains(framework_crate)
    }
}

/// A per-framework, deterministic `:EntryPoint` recogniser (RFC-049
/// §3.1). Each detector recognises one framework's entry idiom in the
/// Rust HIR AST and projects it onto the existing `:EntryPoint`
/// vocabulary; it adds no new parse pass.
///
/// The trait is generic over the concrete `DB: HirDatabase` rather than
/// taking `&dyn HirDatabase` because that trait is not object-safe
/// (RFC-029 §A1.2); `Box<dyn FrameworkDetector<DB>>` is still object-
/// safe (no generic methods, no associated types, `&self` receivers),
/// so the registry holds trait objects and adding a framework is a
/// `Vec` registration.
pub(crate) trait FrameworkDetector<DB: HirDatabase> {
    /// Whether this detector's framework is present in `manifest` and so
    /// the detector should run. The registry never invokes
    /// [`detect`](FrameworkDetector::detect) on a detector that reports
    /// `false` — the RFC-049 §3.1 "inert off-framework" guarantee.
    fn present(&self, manifest: &Manifest) -> bool;

    /// Recognise this framework's entry idioms in `source_file` and
    /// return the `:EntryPoint` nodes plus their `EXPOSES` /
    /// `REGISTERS_PARAM` edges. Handlers that do not resolve to a real
    /// `:Item` are dropped, not synthesised (RFC-049 §3.3).
    fn detect(
        &self,
        sema: &Semantics<'_, DB>,
        source_file: &ast::SourceFile,
        file_path: &Path,
    ) -> (Vec<Node>, Vec<Edge>);
}

/// The ordered set of framework detectors the entry-point pass runs.
///
/// Registration order does not affect output — [`extract_entry_points`]
/// sorts the merged fact set — but is kept stable for readability. The
/// registry is the OCP seam: a new framework is a `Box::new(..)` pushed
/// into [`rust_default`](FrameworkRegistry::rust_default), never a new
/// dispatch arm.
///
/// [`extract_entry_points`]: super::extract_entry_points
pub(crate) struct FrameworkRegistry<DB: HirDatabase> {
    detectors: Vec<Box<dyn FrameworkDetector<DB>>>,
}

impl<DB: HirDatabase> FrameworkRegistry<DB> {
    /// Build a registry over an explicit detector list. Used by
    /// [`rust_default`](FrameworkRegistry::rust_default) and by the
    /// dispatch unit test.
    pub(crate) fn new(detectors: Vec<Box<dyn FrameworkDetector<DB>>>) -> Self {
        Self { detectors }
    }

    /// The v0.2 Rust framework detector set — the exact recogniser
    /// coverage the pre-49-0 inline scan emitted: clap (`cli_command`),
    /// MCP (`mcp_tool`), axum/actix (`http_route`), cron (`cron_job`),
    /// and websocket. Test/bench classification is not a framework and
    /// stays out of this registry (see
    /// [`super::scan_test_bench_fns`]).
    pub(crate) fn rust_default() -> Self {
        let detectors: Vec<Box<dyn FrameworkDetector<DB>>> = vec![
            Box::new(ClapDetector),
            Box::new(McpDetector),
            Box::new(HttpRouteDetector),
            Box::new(CronDetector),
            Box::new(WebsocketDetector),
        ];
        Self::new(detectors)
    }

    /// Run every present detector over `source_file`, merging their
    /// emitted facts. Inert detectors (whose `present(manifest)` is
    /// `false`) are skipped and never reach `detect` — the RFC-049
    /// §3.1 off-framework guarantee.
    pub(crate) fn detect_file(
        &self,
        manifest: &Manifest,
        sema: &Semantics<'_, DB>,
        source_file: &ast::SourceFile,
        file_path: &Path,
    ) -> (Vec<Node>, Vec<Edge>) {
        let mut nodes = Vec::new();
        let mut edges = Vec::new();
        for detector in &self.detectors {
            if !detector.present(manifest) {
                continue;
            }
            let (mut detector_nodes, mut detector_edges) =
                detector.detect(sema, source_file, file_path);
            nodes.append(&mut detector_nodes);
            edges.append(&mut detector_edges);
        }
        (nodes, edges)
    }
}

/// clap-derive CLI commands: `struct` / `enum` carrying
/// `#[derive(Parser|Subcommand)]` → `cli_command` `:EntryPoint` plus
/// one `REGISTERS_PARAM` per `#[arg]` field / per variant.
struct ClapDetector;

impl<DB: HirDatabase> FrameworkDetector<DB> for ClapDetector {
    fn present(&self, manifest: &Manifest) -> bool {
        // RFC-049 §3.1/§4: run only where the workspace depends on clap.
        manifest.depends_on("clap")
    }

    fn detect(
        &self,
        sema: &Semantics<'_, DB>,
        source_file: &ast::SourceFile,
        file_path: &Path,
    ) -> (Vec<Node>, Vec<Edge>) {
        let mut nodes = Vec::new();
        let mut edges = Vec::new();
        for descendant in source_file.syntax().descendants() {
            match descendant.kind() {
                SyntaxKind::STRUCT => {
                    if let Some(strukt) = ast::Struct::cast(descendant) {
                        if has_clap_derive(&strukt) {
                            if let Some((name, qname)) = super::struct_name_and_qname(sema, &strukt)
                            {
                                super::emit(
                                    &mut nodes,
                                    &mut edges,
                                    &qname,
                                    &name,
                                    "cli_command",
                                    file_path,
                                    None,
                                );
                                emit_clap_struct_registers_param(&qname, &strukt, &mut edges);
                            }
                        }
                    }
                }
                SyntaxKind::ENUM => {
                    if let Some(enum_) = ast::Enum::cast(descendant) {
                        if has_clap_derive(&enum_) {
                            if let Some((name, qname)) = super::enum_name_and_qname(sema, &enum_) {
                                super::emit(
                                    &mut nodes,
                                    &mut edges,
                                    &qname,
                                    &name,
                                    "cli_command",
                                    file_path,
                                    None,
                                );
                                emit_clap_enum_registers_param(&qname, &enum_, &mut edges);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        (nodes, edges)
    }
}

/// MCP tools: `fn` carrying an attribute whose last path segment is
/// `tool` → `mcp_tool` `:EntryPoint` plus one `REGISTERS_PARAM` per
/// non-`self` param. `#[tool]` wins over test/bench classification
/// (RFC-042 §3.1) — the test/bench pass skips `#[tool]` fns so this
/// detector is the sole emitter for them.
struct McpDetector;

impl<DB: HirDatabase> FrameworkDetector<DB> for McpDetector {
    fn present(&self, _manifest: &Manifest) -> bool {
        true
    }

    fn detect(
        &self,
        sema: &Semantics<'_, DB>,
        source_file: &ast::SourceFile,
        file_path: &Path,
    ) -> (Vec<Node>, Vec<Edge>) {
        let mut nodes = Vec::new();
        let mut edges = Vec::new();
        for descendant in source_file.syntax().descendants() {
            if descendant.kind() != SyntaxKind::FN {
                continue;
            }
            let Some(fn_ast) = ast::Fn::cast(descendant) else {
                continue;
            };
            if !has_tool_attr(&fn_ast) {
                continue;
            }
            if let Some((name, qname)) = super::fn_name_and_qname(sema, &fn_ast) {
                super::emit(
                    &mut nodes, &mut edges, &qname, &name, "mcp_tool", file_path, None,
                );
                emit_mcp_registers_param(&qname, &fn_ast, &mut edges);
            }
        }
        (nodes, edges)
    }
}

/// axum / actix HTTP routes: `route|get|post|put|delete|patch|nest`
/// method calls with a literal `/`-path and a resolvable handler →
/// `http_route` `:EntryPoint`. Delegates to the parent's shipped
/// [`super::http_route::classify_http_route_method_call`] recogniser.
struct HttpRouteDetector;

impl<DB: HirDatabase> FrameworkDetector<DB> for HttpRouteDetector {
    fn present(&self, manifest: &Manifest) -> bool {
        // RFC-049 §3.1/§4: run only where the workspace depends on axum
        // or actix-web (normalized to `actix_web`).
        manifest.depends_on("axum") || manifest.depends_on("actix_web")
    }

    fn detect(
        &self,
        sema: &Semantics<'_, DB>,
        source_file: &ast::SourceFile,
        file_path: &Path,
    ) -> (Vec<Node>, Vec<Edge>) {
        let mut nodes = Vec::new();
        let mut edges = Vec::new();
        for descendant in source_file.syntax().descendants() {
            if descendant.kind() != SyntaxKind::METHOD_CALL_EXPR {
                continue;
            }
            if let Some(method_call) = ast::MethodCallExpr::cast(descendant) {
                super::http_route::classify_http_route_method_call(
                    sema,
                    &method_call,
                    file_path,
                    &mut nodes,
                    &mut edges,
                );
            }
        }
        (nodes, edges)
    }
}

/// cron jobs: `Job::new_async(<cron-lit>, ..)` / `Job::new(..)` call
/// expressions → `cron_job` `:EntryPoint` carrying the literal
/// schedule. Delegates to the parent's shipped
/// [`super::other_kinds::try_emit_cron_job`] recogniser.
struct CronDetector;

impl<DB: HirDatabase> FrameworkDetector<DB> for CronDetector {
    fn present(&self, _manifest: &Manifest) -> bool {
        true
    }

    fn detect(
        &self,
        sema: &Semantics<'_, DB>,
        source_file: &ast::SourceFile,
        file_path: &Path,
    ) -> (Vec<Node>, Vec<Edge>) {
        let mut nodes = Vec::new();
        let mut edges = Vec::new();
        for descendant in source_file.syntax().descendants() {
            if descendant.kind() != SyntaxKind::CALL_EXPR {
                continue;
            }
            if let Some(call) = ast::CallExpr::cast(descendant) {
                super::other_kinds::try_emit_cron_job(
                    sema, &call, file_path, &mut nodes, &mut edges,
                );
            }
        }
        (nodes, edges)
    }
}

/// websocket upgrades: `<expr>.on_upgrade(<handler>)` method calls →
/// `websocket` `:EntryPoint`. Delegates to the parent's shipped
/// [`super::other_kinds::try_emit_websocket`] recogniser.
struct WebsocketDetector;

impl<DB: HirDatabase> FrameworkDetector<DB> for WebsocketDetector {
    fn present(&self, _manifest: &Manifest) -> bool {
        true
    }

    fn detect(
        &self,
        sema: &Semantics<'_, DB>,
        source_file: &ast::SourceFile,
        file_path: &Path,
    ) -> (Vec<Node>, Vec<Edge>) {
        let mut nodes = Vec::new();
        let mut edges = Vec::new();
        for descendant in source_file.syntax().descendants() {
            if descendant.kind() != SyntaxKind::METHOD_CALL_EXPR {
                continue;
            }
            if let Some(method_call) = ast::MethodCallExpr::cast(descendant) {
                super::other_kinds::try_emit_websocket(
                    sema,
                    &method_call,
                    file_path,
                    &mut nodes,
                    &mut edges,
                );
            }
        }
        (nodes, edges)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::Path;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    use cfdb_core::fact::{Edge, Node};
    use ra_ap_hir::db::HirDatabase;
    use ra_ap_hir::Semantics;
    use ra_ap_ide_db::RootDatabase;
    use ra_ap_syntax::ast;
    use ra_ap_syntax::{Edition, SourceFile};

    use super::{FrameworkDetector, FrameworkRegistry, Manifest};

    /// A test double reporting a fixed `present` verdict and recording
    /// whether `detect` was invoked. Lets the dispatch test observe that
    /// only present detectors reach `detect`.
    struct RecordingDetector {
        present: bool,
        invoked: Arc<AtomicBool>,
    }

    impl<DB: HirDatabase> FrameworkDetector<DB> for RecordingDetector {
        fn present(&self, _manifest: &Manifest) -> bool {
            self.present
        }

        fn detect(
            &self,
            _sema: &Semantics<'_, DB>,
            _source_file: &ast::SourceFile,
            _file_path: &Path,
        ) -> (Vec<Node>, Vec<Edge>) {
            self.invoked.store(true, Ordering::SeqCst);
            (Vec::new(), Vec::new())
        }
    }

    /// RFC-049 49-0 unit contract: the registry dispatches an AST to
    /// exactly the detectors whose `present(manifest)` is true; inert
    /// detectors are not invoked.
    #[test]
    fn registry_dispatches_only_to_present_detectors() {
        let present_invoked = Arc::new(AtomicBool::new(false));
        let inert_invoked = Arc::new(AtomicBool::new(false));

        let detectors: Vec<Box<dyn FrameworkDetector<RootDatabase>>> = vec![
            Box::new(RecordingDetector {
                present: true,
                invoked: Arc::clone(&present_invoked),
            }),
            Box::new(RecordingDetector {
                present: false,
                invoked: Arc::clone(&inert_invoked),
            }),
        ];
        let registry = FrameworkRegistry::<RootDatabase>::new(detectors);

        // An empty `RootDatabase` is enough: the doubles ignore the
        // Semantics, so no real HIR query runs. The seam contract under
        // test is the present-gated dispatch, not any recogniser.
        let db = RootDatabase::default();
        let sema = Semantics::new(&db);
        let source_file = SourceFile::parse("", Edition::Edition2021).tree();

        // The recording doubles ignore the manifest — an empty one keeps
        // the test focused on the present-gated dispatch contract.
        let manifest = Manifest {
            dependency_names: BTreeSet::new(),
        };
        let (nodes, edges) =
            registry.detect_file(&manifest, &sema, &source_file, Path::new("lib.rs"));

        assert!(
            present_invoked.load(Ordering::SeqCst),
            "the present detector's detect() must be invoked"
        );
        assert!(
            !inert_invoked.load(Ordering::SeqCst),
            "the inert (present=false) detector's detect() must NOT be invoked"
        );
        assert!(
            nodes.is_empty() && edges.is_empty(),
            "the recording doubles emit no facts"
        );
    }
}

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
//! **Manifest gating is wired but permissive here.** Per RFC-049 §3.1
//! each detector is meant to be gated on its framework's manifest
//! presence (`present(manifest)`), so a detector never runs on a
//! workspace that does not depend on its framework. In 49-0 every
//! shipped Rust detector reports itself unconditionally present — the
//! existing recognisers are attribute-/call-syntactic and the fixture
//! corpus relies on that (fixtures declare the idiom without depending
//! on the real crate). Narrowing each `present` to its framework's
//! manifest key, together with the per-framework recall fixtures that
//! prove no regression, is slices 49-A/49-B (#494 / #495).
//!
//! **Language scoping (RFC-049 §3.1 Q4).** This registry is the *Rust*
//! detector set; its `detect` is parameterised by the Rust HIR AST. The
//! PHP / TypeScript detectors (49-C / 49-D) live in their own extractor
//! crates over their own (tree-sitter) ASTs and never share this trait
//! — a detector never reaches across a language-extractor boundary.

use std::path::Path;

use cfdb_core::fact::{Edge, Node};
use ra_ap_hir::db::HirDatabase;
use ra_ap_hir::Semantics;
use ra_ap_syntax::ast::{self, AstNode};
use ra_ap_syntax::SyntaxKind;

use super::registers_param::{
    emit_clap_enum_registers_param, emit_clap_struct_registers_param, emit_mcp_registers_param,
    has_clap_derive, has_tool_attr,
};

/// The workspace manifest view a [`FrameworkDetector::present`] gate
/// consults (RFC-049 §3.1 — `Cargo.toml [dependencies]`).
///
/// It carries no data in slice 49-0: every shipped Rust detector is
/// unconditionally present (see the module docs), so nothing reads the
/// manifest yet. The dependency-set population and the per-framework
/// `depends_on(...)` gating land with the first real gate in 49-A/49-B.
/// The type exists now so the `present(manifest)` contract — the shape
/// the whole registry dispatches on — is fixed by this slice.
pub(crate) struct Manifest;

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

        let (nodes, edges) =
            registry.detect_file(&Manifest, &sema, &source_file, Path::new("lib.rs"));

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

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
use crate::target_map::EmitCtx;

pub(crate) struct Manifest {
    dependency_names: BTreeSet<String>,
}

impl Manifest {
    pub(crate) fn from_crate_graph<DB: HirDatabase>(
        db: &DB,
        vfs: &Vfs,
        workspace_root: &Path,
    ) -> Self {
        let ws_root_canonical = workspace_root.canonicalize().ok();
        let mut dependency_names = BTreeSet::new();
        for krate in Crate::all(db) {
            if !krate.origin(db).is_local() {
                continue;
            }
            let Some(root_path) = super::vfs_path_to_pathbuf(vfs.file_path(krate.root_file(db)))
            else {
                continue;
            };
            let contained = match (root_path.canonicalize().ok(), &ws_root_canonical) {
                (Some(candidate), Some(root)) => candidate.starts_with(root),
                _ => root_path.starts_with(workspace_root),
            };
            if !contained {
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

    fn depends_on(&self, framework_crate: &str) -> bool {
        self.dependency_names.contains(framework_crate)
    }
}

pub(crate) trait FrameworkDetector<DB: HirDatabase> {
    fn present(&self, manifest: &Manifest) -> bool;

    fn detect(
        &self,
        sema: &Semantics<'_, DB>,
        ctx: &EmitCtx<'_>,
        source_file: &ast::SourceFile,
        file_path: &Path,
    ) -> (Vec<Node>, Vec<Edge>);
}

pub(crate) struct FrameworkRegistry<DB: HirDatabase> {
    detectors: Vec<Box<dyn FrameworkDetector<DB>>>,
}

impl<DB: HirDatabase> FrameworkRegistry<DB> {
    pub(crate) fn new(detectors: Vec<Box<dyn FrameworkDetector<DB>>>) -> Self {
        Self { detectors }
    }

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

    pub(crate) fn detect_file(
        &self,
        manifest: &Manifest,
        sema: &Semantics<'_, DB>,
        ctx: &EmitCtx<'_>,
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
                detector.detect(sema, ctx, source_file, file_path);
            nodes.append(&mut detector_nodes);
            edges.append(&mut detector_edges);
        }
        (nodes, edges)
    }
}

struct ClapDetector;

impl<DB: HirDatabase> FrameworkDetector<DB> for ClapDetector {
    fn present(&self, manifest: &Manifest) -> bool {
        manifest.depends_on("clap")
    }

    fn detect(
        &self,
        sema: &Semantics<'_, DB>,
        ctx: &EmitCtx<'_>,
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
                            if let Some(handler) = super::struct_handler(sema, ctx, &strukt) {
                                super::emit(
                                    &mut nodes,
                                    &mut edges,
                                    &handler,
                                    "cli_command",
                                    file_path,
                                    None,
                                );
                                emit_clap_struct_registers_param(
                                    &handler.identity(),
                                    &strukt,
                                    &mut edges,
                                );
                            }
                        }
                    }
                }
                SyntaxKind::ENUM => {
                    if let Some(enum_) = ast::Enum::cast(descendant) {
                        if has_clap_derive(&enum_) {
                            if let Some(handler) = super::enum_handler(sema, ctx, &enum_) {
                                super::emit(
                                    &mut nodes,
                                    &mut edges,
                                    &handler,
                                    "cli_command",
                                    file_path,
                                    None,
                                );
                                emit_clap_enum_registers_param(
                                    &handler.identity(),
                                    &enum_,
                                    &mut edges,
                                );
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

struct McpDetector;

impl<DB: HirDatabase> FrameworkDetector<DB> for McpDetector {
    fn present(&self, _manifest: &Manifest) -> bool {
        true
    }

    fn detect(
        &self,
        sema: &Semantics<'_, DB>,
        ctx: &EmitCtx<'_>,
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
            if let Some(handler) = super::fn_handler(sema, ctx, &fn_ast) {
                super::emit(
                    &mut nodes, &mut edges, &handler, "mcp_tool", file_path, None,
                );
                emit_mcp_registers_param(&handler.identity(), &fn_ast, &mut edges);
            }
        }
        (nodes, edges)
    }
}

struct HttpRouteDetector;

impl<DB: HirDatabase> FrameworkDetector<DB> for HttpRouteDetector {
    fn present(&self, manifest: &Manifest) -> bool {
        manifest.depends_on("axum") || manifest.depends_on("actix_web")
    }

    fn detect(
        &self,
        sema: &Semantics<'_, DB>,
        ctx: &EmitCtx<'_>,
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
                    ctx,
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

struct CronDetector;

impl<DB: HirDatabase> FrameworkDetector<DB> for CronDetector {
    fn present(&self, _manifest: &Manifest) -> bool {
        true
    }

    fn detect(
        &self,
        sema: &Semantics<'_, DB>,
        ctx: &EmitCtx<'_>,
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
                    sema, ctx, &call, file_path, &mut nodes, &mut edges,
                );
            }
        }
        (nodes, edges)
    }
}

struct WebsocketDetector;

impl<DB: HirDatabase> FrameworkDetector<DB> for WebsocketDetector {
    fn present(&self, _manifest: &Manifest) -> bool {
        true
    }

    fn detect(
        &self,
        sema: &Semantics<'_, DB>,
        ctx: &EmitCtx<'_>,
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
                    ctx,
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

    use super::{EmitCtx, FrameworkDetector, FrameworkRegistry, Manifest};
    use crate::target_map::TargetRootMap;
    use ra_ap_vfs::Vfs;

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
            _ctx: &EmitCtx<'_>,
            _source_file: &ast::SourceFile,
            _file_path: &Path,
        ) -> (Vec<Node>, Vec<Edge>) {
            self.invoked.store(true, Ordering::SeqCst);
            (Vec::new(), Vec::new())
        }
    }

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

        let db = RootDatabase::default();
        let sema = Semantics::new(&db);
        let source_file = SourceFile::parse("", Edition::Edition2021).tree();

        let manifest = Manifest {
            dependency_names: BTreeSet::new(),
        };
        let vfs = Vfs::default();
        let targets = TargetRootMap::default();
        let ctx = EmitCtx {
            vfs: &vfs,
            targets: &targets,
        };
        let (nodes, edges) =
            registry.detect_file(&manifest, &sema, &ctx, &source_file, Path::new("lib.rs"));

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

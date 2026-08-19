use std::fs;
use std::path::{Path, PathBuf};

use cfdb_core::fact::{Edge, Node, PropValue, Props};
use cfdb_core::schema::Label;
use cfdb_lang::{LanguageError, LanguageProducer};
use tree_sitter::Parser;

mod call_walker;
mod emit;
mod methods;

const PRODUCER_NAME: &str = "typescript";

const TSCONFIG_JSON: &str = "tsconfig.json";
const PACKAGE_JSON: &str = "package.json";

const SKIPPED_DIRS: &[&str] = &["node_modules", "dist", "build"];

pub struct TypeScriptProducer;

impl LanguageProducer for TypeScriptProducer {
    fn name(&self) -> &'static str {
        PRODUCER_NAME
    }

    fn detect(&self, workspace_root: &Path) -> bool {
        workspace_root.join(TSCONFIG_JSON).is_file() && workspace_root.join(PACKAGE_JSON).is_file()
    }

    fn produce(&self, workspace_root: &Path) -> Result<(Vec<Node>, Vec<Edge>), LanguageError> {
        if !self.detect(workspace_root) {
            return Err(LanguageError::NotDetected {
                producer: PRODUCER_NAME,
                reason: format!(
                    "missing `{TSCONFIG_JSON}` and/or `{PACKAGE_JSON}` at workspace root"
                ),
            });
        }

        let workspace_root = cfdb_lang::canonical_workspace_root(workspace_root)?;
        let workspace_root = workspace_root.as_path();

        let crate_name = derive_crate_name(workspace_root);
        let crate_id = format!("crate:{crate_name}");

        let mut nodes: Vec<Node> = Vec::new();
        let mut edges: Vec<Edge> = Vec::new();

        nodes.push(Node {
            id: crate_id.clone(),
            label: Label::new(Label::CRATE),
            props: {
                let mut p = Props::new();
                p.insert("name".into(), PropValue::Str(crate_name.clone()));
                p.insert("language".into(), PropValue::Str(PRODUCER_NAME.into()));
                p.insert("is_workspace_member".into(), PropValue::Bool(true));
                p.insert("published_language".into(), PropValue::Bool(false));
                p
            },
        });

        let ts_files = collect_ts_files(workspace_root).map_err(LanguageError::Io)?;

        let mut parser = Parser::new();
        let language: tree_sitter::Language = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();
        parser
            .set_language(&language)
            .map_err(|e| LanguageError::Parse {
                producer: PRODUCER_NAME,
                message: format!("set_language(LANGUAGE_TYPESCRIPT): {e}"),
            })?;

        let mut pending_implements: Vec<(String, String)> = Vec::new();
        for file_path in &ts_files {
            walk_file(
                &mut parser,
                workspace_root,
                file_path,
                &crate_name,
                &crate_id,
                &mut nodes,
                &mut edges,
                &mut pending_implements,
            )?;
        }

        emit::resolve_implements(pending_implements, &nodes, &mut edges);

        nodes.sort_by(|a, b| a.sort_key().cmp(&b.sort_key()));
        edges.sort_by(|a, b| a.sort_key().cmp(&b.sort_key()));

        Ok((nodes, edges))
    }
}

fn derive_crate_name(workspace_root: &Path) -> String {
    workspace_root
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("ts_workspace")
        .to_string()
}

fn collect_ts_files(workspace_root: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut acc = Vec::new();
    visit_dir(workspace_root, &mut acc)?;
    acc.sort();
    Ok(acc)
}

fn visit_dir(dir: &Path, acc: &mut Vec<PathBuf>) -> std::io::Result<()> {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            let dir_name = path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or_default();
            if SKIPPED_DIRS.contains(&dir_name) || dir_name.starts_with('.') {
                continue;
            }
            visit_dir(&path, acc)?;
        } else if file_type.is_file()
            && path
                .extension()
                .and_then(|s| s.to_str())
                .map(|ext| ext == "ts")
                .unwrap_or(false)
            && !path
                .file_name()
                .and_then(|s| s.to_str())
                .map(|n| n.ends_with(".d.ts"))
                .unwrap_or(false)
        {
            acc.push(path);
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn walk_file(
    parser: &mut Parser,
    workspace_root: &Path,
    file_path: &Path,
    crate_name: &str,
    crate_id: &str,
    nodes: &mut Vec<Node>,
    edges: &mut Vec<Edge>,
    pending_implements: &mut Vec<(String, String)>,
) -> Result<(), LanguageError> {
    let source = fs::read_to_string(file_path).map_err(LanguageError::Io)?;
    let tree = parser.parse(&source, None).ok_or(LanguageError::Parse {
        producer: PRODUCER_NAME,
        message: format!("tree-sitter returned None for {}", file_path.display()),
    })?;

    let rel_path = cfdb_lang::workspace_relative(file_path, workspace_root, PRODUCER_NAME)?;
    let module_qpath = ts_module_qpath(&rel_path);
    let module_id = format!("module:{crate_name}::{module_qpath}");

    nodes.push(Node {
        id: module_id.clone(),
        label: Label::new(Label::MODULE),
        props: {
            let mut p = Props::new();
            p.insert("qpath".into(), PropValue::Str(module_qpath.clone()));
            p.insert("file".into(), PropValue::Str(rel_path.clone()));
            p.insert("crate".into(), PropValue::Str(crate_name.to_string()));
            p
        },
    });

    let root = tree.root_node();
    let bytes = source.as_bytes();
    emit::walk_program(
        root,
        bytes,
        crate_name,
        crate_id,
        &module_qpath,
        &module_id,
        &rel_path,
        nodes,
        edges,
        pending_implements,
    );
    Ok(())
}

fn ts_module_qpath(rel_path: &str) -> String {
    let trimmed = rel_path.strip_suffix(".ts").unwrap_or(rel_path);
    trimmed
        .split(['/', '\\'])
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(".")
}

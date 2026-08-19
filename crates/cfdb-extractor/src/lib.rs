#![allow(unknown_lints)]
#![deny(non_exhaustive_omitted_patterns)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use cargo_metadata::MetadataCommand;
use cfdb_core::fact::{Edge, Node};
use cfdb_core::ContextSource;
use thiserror::Error;

mod attrs;
mod call_visitor;
mod const_table;
mod crate_tier;
mod emitter;
mod file_walker;
mod item_visitor;
mod literal_visitor;
mod macro_tokens;
mod match_visitor;
mod resolver;
mod synthesize;
mod type_render;
mod workspace_nodes;

pub(crate) use emitter::Emitter;

use cfdb_concepts::{load_concept_overrides, load_published_language_crates, ContextMeta};
use workspace_nodes::{emit_context_node, emit_crate_and_walk_targets, seed_declared_contexts};

#[derive(Debug, Error)]
pub enum ExtractError {
    #[error("cargo metadata: {0}")]
    Metadata(String),

    #[error("io error reading {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("parse error in {path}: {message}")]
    Parse { path: PathBuf, message: String },

    #[error("concept overrides: {0}")]
    Concepts(String),

    #[error("crate_tier: cycle in the intra-workspace normal-dependency DAG involving crate `{0}` (RFC-050 §3.2 — normal deps must form a DAG)")]
    CrateTierCycle(String),

    #[error("cannot canonicalize workspace root {path}: {source}")]
    WorkspaceRoot {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error(
        "file {file} lies outside the canonical workspace root {workspace_root} — refusing to \
         silently emit an absolute :File.path (issue #527: every emitted file path must be \
         workspace-relative; a residual strip_prefix mismatch is a hard error, not a warned-\
         and-shipped absolute path)"
    )]
    PathNotInWorkspace {
        file: PathBuf,
        workspace_root: PathBuf,
    },
}

pub struct RustProducer;

impl cfdb_lang::LanguageProducer for RustProducer {
    fn name(&self) -> &'static str {
        "rust"
    }

    fn detect(&self, workspace_root: &Path) -> bool {
        workspace_root.join("Cargo.toml").is_file()
    }

    fn produce(
        &self,
        workspace_root: &Path,
    ) -> Result<(Vec<Node>, Vec<Edge>), cfdb_lang::LanguageError> {
        extract_workspace(workspace_root).map_err(|e| cfdb_lang::LanguageError::Parse {
            producer: "rust",
            message: e.to_string(),
        })
    }
}

pub fn extract_workspace(workspace_root: &Path) -> Result<(Vec<Node>, Vec<Edge>), ExtractError> {
    extract_workspace_profiled(workspace_root, &mut |_| {})
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtractPhaseMarker {
    CargoMetadataStart,
    SynWalkStart,
    DeferredResolveStart,
    Finished,
}

pub fn extract_workspace_profiled(
    workspace_root: &Path,
    observe: &mut dyn FnMut(ExtractPhaseMarker),
) -> Result<(Vec<Node>, Vec<Edge>), ExtractError> {
    let workspace_root_buf =
        workspace_root
            .canonicalize()
            .map_err(|e| ExtractError::WorkspaceRoot {
                path: workspace_root.to_path_buf(),
                source: e,
            })?;
    let workspace_root: &Path = &workspace_root_buf;

    observe(ExtractPhaseMarker::CargoMetadataStart);
    let manifest = workspace_root.join("Cargo.toml");
    let metadata = MetadataCommand::new()
        .manifest_path(&manifest)
        .no_deps()
        .exec()
        .map_err(|e| ExtractError::Metadata(e.to_string()))?;

    observe(ExtractPhaseMarker::SynWalkStart);

    let overrides = load_concept_overrides(workspace_root)
        .map_err(|e| ExtractError::Concepts(e.to_string()))?;

    let published_language = load_published_language_crates(workspace_root)
        .map_err(|e| ExtractError::Concepts(e.to_string()))?;

    let mut emitter = Emitter::new();

    let mut contexts_seen: BTreeMap<String, (ContextMeta, ContextSource)> =
        seed_declared_contexts(&overrides);

    let packages = metadata.workspace_packages();
    let crate_tiers = crate_tier::compute_crate_tiers(&packages)?;

    for package in packages.iter().copied() {
        emit_crate_and_walk_targets(
            &mut emitter,
            package,
            &crate_tiers,
            &overrides,
            &published_language,
            &mut contexts_seen,
            workspace_root,
        )?;
    }

    for (name, (meta, source)) in &contexts_seen {
        emit_context_node(&mut emitter, name, meta, *source);
    }

    observe(ExtractPhaseMarker::DeferredResolveStart);

    resolver::resolve_deferred_returns(&mut emitter);
    resolver::resolve_deferred_type_of(&mut emitter);
    resolver::resolve_deferred_match_targets(&mut emitter);
    synthesize::synthesize_referenced_items(&mut emitter, &overrides);

    let (mut nodes, mut edges) = emitter.finish();
    nodes.sort_by(|a, b| a.sort_key().cmp(&b.sort_key()));
    edges.sort_by(|a, b| a.sort_key().cmp(&b.sort_key()));

    observe(ExtractPhaseMarker::Finished);
    Ok((nodes, edges))
}

#[cfg(test)]
mod tests;

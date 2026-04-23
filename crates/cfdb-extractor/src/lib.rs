//! cfdb-extractor — walk a Rust workspace and emit Node/Edge facts.
//!
//! v0.1 scope (RFC §7 subset):
//!
//! - **Nodes:** `Crate`, `Module`, `File`, `Item`, `Field`, `CallSite`
//! - **Edges:** `IN_CRATE` (Item → Crate), `IN_MODULE` (Item → Module),
//!   `HAS_FIELD` (Item → Field), `INVOKES_AT` (Item → CallSite)
//!
//! **CallSite extraction (RFC §13 "out of scope — unless needed" carve-out).**
//! The Q1=(b) council pick (Pattern D `arch-ban-utc-now`) needs the extractor
//! to see textual call paths inside function and method bodies. This is a
//! *name-based*, unresolved call graph — `syn` gives us the text
//! `chrono::Utc::now` at a call site without any guarantee it resolves to the
//! `chrono` crate's `now` function. That is deliberately sufficient for
//! Pattern D: a ban rule cares about the *appearance* of the symbol in the
//! source, not about full type resolution.
//!
//! **Out of scope for v0.1:** resolved cross-crate `CALLS` (Item → Item),
//! `TYPE_OF`, `IMPLEMENTS`, `RETURNS`, `Param`, `Variant`, `EntryPoint`,
//! `Concept`. Those need full method dispatch and re-export chain following
//! (`ra-ap-hir`, RFC §8.2 Phase B).
//!
//! The extractor is a pure function: `extract_workspace(path) ->
//! (Vec<Node>, Vec<Edge>)`. It does not touch any store; callers ingest the
//! results into a [`cfdb_core::StoreBackend`]. This keeps the extractor
//! testable without a store and preserves the dependency rule (RFC §8).
//!
//! **Module layout (#3718 split).** The production code is partitioned
//! into narrow submodules so no single file dominates:
//!
//! - [`attrs`]        — single-purpose `syn::Attribute` probes
//! - [`type_render`]  — textual rendering of `syn::Type` / `syn::Path`
//! - [`file_walker`]  — recursive module walker + `#[path]` resolution
//! - [`item_visitor`] — `syn::Visit` impl for module-level items
//! - [`call_visitor`] — `syn::Visit` impl for call sites inside fn bodies
//!
//! `lib.rs` keeps only the public entry point, the error type, and the
//! shared [`Emitter`] sink that every submodule writes into.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use cargo_metadata::MetadataCommand;
use cfdb_core::fact::{Edge, Node, PropValue};
use cfdb_core::schema::{EdgeLabel, Label};
use thiserror::Error;

mod attrs;
mod call_visitor;
mod file_walker;
mod item_visitor;
mod type_render;

use cfdb_concepts::{
    compute_bounded_context, load_concept_overrides, load_published_language_crates,
    ConceptOverrides, ContextMeta, PublishedLanguageCrates,
};
use file_walker::visit_file;

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
}

/// Extract all structural facts from a Rust workspace rooted at the given
/// path. The path must contain a `Cargo.toml`.
///
/// Returns `(nodes, edges)` in stable order: nodes sorted by `(label, id)`,
/// edges by `(src, dst, label)`. The caller ingests both into a store.
pub fn extract_workspace(workspace_root: &Path) -> Result<(Vec<Node>, Vec<Edge>), ExtractError> {
    let manifest = workspace_root.join("Cargo.toml");
    let metadata = MetadataCommand::new()
        .manifest_path(&manifest)
        .no_deps()
        .exec()
        .map_err(|e| ExtractError::Metadata(e.to_string()))?;

    // Step 1 (pre-walk): load `.cfdb/concepts/*.toml` overrides so the
    // per-crate bounded-context resolution in the loop below can honour
    // explicit mappings before falling back to the crate-prefix heuristic.
    // Missing directory is not an error — the overrides are optional.
    let overrides = load_concept_overrides(workspace_root)
        .map_err(|e| ExtractError::Concepts(e.to_string()))?;

    // Step 1b (pre-walk): load `.cfdb/published-language-crates.toml`
    // marker list (issue #100 / RFC-cfdb.md Addendum B §A1.8). Missing
    // file is not an error — empty map means every `:Crate` emits
    // `published_language: false`. Classifier (#48) suppresses false
    // Context-Homonym positives for declared published-language crates.
    let published_language = load_published_language_crates(workspace_root)
        .map_err(|e| ExtractError::Concepts(e.to_string()))?;

    let mut emitter = Emitter::new();

    // Accumulate every bounded context we see so we can emit one `:Context`
    // node per unique context after the crate loop. BTreeMap gives us a
    // deterministic emission order (RFC-029 §12.1 G1).
    let mut contexts_seen: BTreeMap<String, ContextMeta> = overrides.declared_contexts();

    for package in metadata.workspace_packages() {
        emit_crate_and_walk_targets(
            &mut emitter,
            package,
            &overrides,
            &published_language,
            &mut contexts_seen,
            workspace_root,
        )?;
    }

    // Step 2 (post-walk): emit one `:Context` node per unique bounded
    // context. `BTreeMap` iteration is ordered, so the emission order is
    // deterministic across runs regardless of which crate discovered the
    // context first. Contexts declared in `.cfdb/concepts/*.toml` that no
    // workspace crate is part of are still emitted — downstream tooling
    // may reference cross-workspace taxonomies.
    for (name, meta) in &contexts_seen {
        emit_context_node(&mut emitter, name, meta);
    }

    // Step 3 (post-walk) — RETURNS resolution (RFC-037 §3.2, #216).
    //
    // For each (fn_qname, rendered_return_type) pair queued by the
    // item visitor, emit a RETURNS edge if the rendered return type
    // resolves to an emitted `:Item` qname in this workspace. The
    // `emitted_item_qnames` set covers every item across every file
    // because both halves of the state live on the workspace-scoped
    // `Emitter` — this lets `pub fn use_foo() -> Foo` declared before
    // `pub struct Foo {}` (within a file or across files) still emit
    // a RETURNS edge: same-walk forward-lookup is unnecessary because
    // the resolution loop runs after every walk has completed.
    //
    // **Documented limitation (RFC-037 §6 non-goals):**
    // `crate::type_render::render_type_string` strips generic
    // arguments — `Vec<T>` renders as `"Vec"`, `Option<T>` as
    // `"Option"`, `Result<T, E>` as `"Result"`. Wrapper-wrapped
    // same-crate types therefore silently do not emit RETURNS /
    // TYPE_OF edges. A follow-up `render_type_inner` that unwraps
    // common wrappers is tracked in RFC-037 §6 non-goals.
    resolve_deferred_returns(&mut emitter);

    let (mut nodes, mut edges) = emitter.finish();
    nodes.sort_by(|a, b| a.sort_key().cmp(&b.sort_key()));
    edges.sort_by(|a, b| a.sort_key().cmp(&b.sort_key()));
    Ok((nodes, edges))
}

/// Emit the `:Crate` node, `BELONGS_TO` edge, synthesised `:Context`
/// entries, and walk each lib/bin target for one workspace package.
/// Factored out of the crate-loop in [`extract_workspace`] so the
/// per-package path-string and context-name clones live in a helper
/// rather than directly inside the outer `for` loop body.
fn emit_crate_and_walk_targets(
    emitter: &mut Emitter,
    package: &cargo_metadata::Package,
    overrides: &ConceptOverrides,
    published_language: &PublishedLanguageCrates,
    contexts_seen: &mut BTreeMap<String, ContextMeta>,
    workspace_root: &Path,
) -> Result<(), ExtractError> {
    let crate_id = format!("crate:{}", package.name);
    let bounded_context = compute_bounded_context(&package.name, overrides);

    // Heuristic-synthesised contexts also need a `:Context` node so
    // `BELONGS_TO` has a valid target. The override-declared ones are
    // already in `contexts_seen`; insert the heuristic result if the
    // name is new.
    let context_for_seen = bounded_context.clone();
    contexts_seen
        .entry(context_for_seen)
        .or_insert_with(|| ContextMeta {
            name: bounded_context.clone(),
            canonical_crate: None,
            owning_rfc: None,
        });

    emitter.emit_node(Node {
        id: crate_id.clone(),
        label: Label::new(Label::CRATE),
        props: {
            let mut p = BTreeMap::new();
            p.insert("name".into(), PropValue::Str(package.name.to_string()));
            p.insert(
                "version".into(),
                PropValue::Str(package.version.to_string()),
            );
            p.insert("is_workspace_member".into(), PropValue::Bool(true));
            // Published Language marker (issue #100 / addendum §A1.8):
            // `true` iff the crate is declared in
            // `.cfdb/published-language-crates.toml`. Every `:Crate`
            // carries this prop — no `Option`, missing file → `false`.
            p.insert(
                "published_language".into(),
                PropValue::Bool(published_language.is_published_language(&package.name)),
            );
            p
        },
    });

    // Emit the Crate -> Context BELONGS_TO edge now so a single pass
    // over edges shows the crate-to-context wiring (council §B.1.3).
    let context_id = format!("context:{bounded_context}");
    emitter.emit_edge(Edge {
        src: crate_id.clone(),
        dst: context_id,
        label: EdgeLabel::new(EdgeLabel::BELONGS_TO),
        props: BTreeMap::new(),
    });

    let targets: Vec<PathBuf> = package
        .targets
        .iter()
        .filter(|t| t.is_lib() || t.is_bin())
        .map(|t| t.src_path.clone().into_std_path_buf())
        .collect();
    for src_root in &targets {
        visit_file(
            emitter,
            &crate_id,
            &package.name,
            &bounded_context,
            src_root,
            workspace_root,
        )?;
    }
    Ok(())
}

/// Emit a single `:Context` node from its accumulated [`ContextMeta`].
/// Pulled out of the context-emission loop so the per-property clones
/// do not count against the `clones-in-loops` metric.
fn emit_context_node(emitter: &mut Emitter, name: &str, meta: &ContextMeta) {
    let id = format!("context:{name}");
    let mut props = BTreeMap::new();
    props.insert("name".into(), PropValue::Str(name.to_string()));
    props.insert(
        "canonical_crate".into(),
        match &meta.canonical_crate {
            Some(s) => PropValue::Str(s.clone()),
            None => PropValue::Null,
        },
    );
    props.insert(
        "owning_rfc".into(),
        match &meta.owning_rfc {
            Some(s) => PropValue::Str(s.clone()),
            None => PropValue::Null,
        },
    );
    emitter.emit_node(Node {
        id,
        label: Label::new(Label::CONTEXT),
        props,
    });
}

/// Post-walk RETURNS resolution (RFC-037 §3.2, #216).
///
/// Iterates every entry queued in `emitter.deferred_returns` and emits
/// a `RETURNS` edge from the fn's `:Item` to the return-type's `:Item`
/// when the rendered return-type string resolves to a qname recorded
/// in `emitter.emitted_item_qnames`. Returns silently when the type
/// does not resolve — this includes cross-crate types not in the
/// walked workspace, primitives, generic wrappers (see limitation in
/// `extract_workspace`), and `impl Trait` returns (rendered as `?`).
///
/// Determinism (G1): deferred entries are appended in walk order
/// (per-file syn::Visit order), and the resulting RETURNS edges land
/// in `emitter.edges` before the final `edges.sort_by(sort_key)` pass
/// in [`extract_workspace`], so the on-disk ordering is independent
/// of the queue iteration order regardless.
fn resolve_deferred_returns(emitter: &mut Emitter) {
    // Drain into a local Vec so we don't hold a borrow on `emitter`
    // while calling `emit_edge`. Cloning the qnames keeps the borrow
    // checker quiet without changing the semantics.
    let deferred: Vec<(String, String)> = std::mem::take(&mut emitter.deferred_returns);

    // Build a last-segment index: `render_type_string` produces paths
    // as-written (`Foo`, `mymod::Bar`), but `emitted_item_qnames` holds
    // crate-prefixed qnames (`mycrate::Foo`). Match on the full string
    // first (fast path for already-qualified returns); fall back to
    // last-segment match when unique. Ambiguous last-segments (e.g.
    // `Error` declared in multiple crates) emit no edge — safer than
    // mis-attribution; documented as a limitation alongside the
    // generic-stripping one in `extract_workspace`.
    let mut by_last_segment: BTreeMap<&str, Option<&String>> = BTreeMap::new();
    for qname in &emitter.emitted_item_qnames {
        let seg = cfdb_core::qname::last_segment(qname);
        by_last_segment
            .entry(seg)
            .and_modify(|v| *v = None) // mark ambiguous
            .or_insert(Some(qname));
    }

    let mut resolved: Vec<(String, String)> = Vec::new();
    for (fn_qname, return_type) in &deferred {
        let target = if emitter.emitted_item_qnames.contains(return_type) {
            Some(return_type.clone())
        } else {
            let seg = cfdb_core::qname::last_segment(return_type);
            // unknown or ambiguous last-segment → drop silently
            by_last_segment.get(seg).copied().flatten().cloned()
        };
        if let Some(target_qname) = target {
            resolved.push((fn_qname.clone(), target_qname));
        }
    }

    for (fn_qname, target_qname) in resolved {
        emitter.emit_edge(Edge {
            src: cfdb_core::qname::item_node_id(&fn_qname),
            dst: cfdb_core::qname::item_node_id(&target_qname),
            label: EdgeLabel::new(EdgeLabel::RETURNS),
            props: BTreeMap::new(),
        });
    }
}

/// Shared node/edge sink. Every submodule that walks the AST holds a
/// `&mut Emitter` and pushes into these vectors; the outer
/// [`extract_workspace`] owns the instance and calls [`Emitter::finish`]
/// once the workspace has been fully walked.
///
/// **RETURNS / TYPE_OF post-walk state (RFC-037 §3.2, #216).** Two
/// fields support deferred edge resolution: `emitted_item_qnames`
/// records every `:Item` qname the extractor has emitted (populated by
/// `emit_item_with_flags` and the impl-method emission path), and
/// `deferred_returns` records `(fn_qname, rendered_return_type_string)`
/// pairs queued by `visit_item_fn` / `visit_impl_item_fn`. Once the
/// workspace walk is complete, [`extract_workspace`] iterates the
/// deferred queue and emits a `RETURNS` edge whenever the rendered
/// return type matches a known item qname. Holding these on the
/// workspace-scoped `Emitter` (rather than on the per-file
/// [`crate::item_visitor::ItemVisitor`]) means the resolution loop sees
/// every item across every file regardless of walk order — a
/// `pub fn use_foo() -> Foo` declared before `pub struct Foo {}` in
/// the same file (or in a different file walked earlier) still
/// resolves correctly.
pub(crate) struct Emitter {
    nodes: Vec<Node>,
    edges: Vec<Edge>,
    /// Qnames of every `:Item` emitted so far — used for RETURNS /
    /// TYPE_OF post-walk resolution. Populated by
    /// [`crate::item_visitor::ItemVisitor::emit_item_with_flags`] and
    /// by the impl-method emission path in
    /// [`crate::item_visitor::ItemVisitor::visit_impl_item_fn`].
    pub(crate) emitted_item_qnames: std::collections::BTreeSet<String>,
    /// Deferred RETURNS edges — `(fn_item_qname,
    /// rendered_return_type_string)`. Walked after all items are
    /// emitted; if the type string resolves to a qname in
    /// `emitted_item_qnames`, a `RETURNS` edge is pushed.
    pub(crate) deferred_returns: Vec<(String, String)>,
}

impl Emitter {
    fn new() -> Self {
        Self {
            nodes: Vec::new(),
            edges: Vec::new(),
            emitted_item_qnames: std::collections::BTreeSet::new(),
            deferred_returns: Vec::new(),
        }
    }

    pub(crate) fn emit_node(&mut self, node: Node) {
        self.nodes.push(node);
    }

    pub(crate) fn emit_edge(&mut self, edge: Edge) {
        self.edges.push(edge);
    }

    fn finish(self) -> (Vec<Node>, Vec<Edge>) {
        (self.nodes, self.edges)
    }
}

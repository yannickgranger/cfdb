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

// RFC-044 §3.7 (slice 044-G): cfdb-core schema enums are `#[non_exhaustive]`.
// Cross-crate `match` sites require `_ =>` arms by E0004; the deny below
// auto-activates when the (nightly-only on rust 1.93)
// `non_exhaustive_omitted_patterns` lint stabilises.
#![allow(unknown_lints)]
#![deny(non_exhaustive_omitted_patterns)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use cargo_metadata::MetadataCommand;
use cfdb_core::fact::{Edge, Node, PropValue};
use cfdb_core::schema::{EdgeLabel, Label};
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
mod resolver;
mod synthesize;
mod type_render;

pub(crate) use emitter::Emitter;

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

    #[error("crate_tier: cycle in the intra-workspace normal-dependency DAG involving crate `{0}` (RFC-050 §3.2 — normal deps must form a DAG)")]
    CrateTierCycle(String),
}

// ---------------------------------------------------------------------------
// `LanguageProducer` impl — RFC-041 Phase 1 / Slice 41-B.
//
// The trait surface lives in `cfdb-lang`; this crate provides the Rust
// reference implementation. `cfdb-cli`'s `available_producers()` registry
// (see `crates/cfdb-cli/src/lang.rs`) is the only place that names this
// concrete type — every other consumer dispatches through
// `&dyn cfdb_lang::LanguageProducer`.
// ---------------------------------------------------------------------------

/// Rust reference implementation of `cfdb_lang::LanguageProducer`.
///
/// Detects a Rust workspace by the presence of a `Cargo.toml` at the
/// workspace root, then delegates to the legacy `extract_workspace`
/// entry point. The `produce` impl wraps `ExtractError` into
/// `LanguageError::Parse { producer: "rust", message: ... }` so callers
/// who dispatch through `&dyn LanguageProducer` get a uniform error
/// type.
///
/// The legacy public function `extract_workspace` is preserved verbatim
/// (same signature, same `ExtractError` return) for backward-compat
/// with qbot-core's `cargo install --git` consumer per RFC-041 §3.3 +
/// EPIC #279. Its deprecation timeline is a deferred architectural
/// decision (RFC-041 §6).
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

/// Extract all structural facts from a Rust workspace rooted at the given
/// path. The path must contain a `Cargo.toml`.
///
/// Returns `(nodes, edges)` in stable order: nodes sorted by `(label, id)`,
/// edges by `(src, dst, label)`. The caller ingests both into a store.
///
/// # Backward-compat shim
///
/// This is the v0.1 entry point preserved verbatim for qbot-core's
/// `cargo install --git` CI consumer (per EPIC #279 / RFC-041 §3.3).
/// New code should dispatch via `&dyn cfdb_lang::LanguageProducer` —
/// see [`RustProducer`] above. The two paths produce byte-identical
/// fact sets; the trait method just wraps `ExtractError` into
/// `LanguageError::Parse`.
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
    //
    // Each entry pairs the metadata with a [`ContextSource`] discriminator
    // (RFC-038 §3.3): `Declared` for contexts pre-seeded from
    // `.cfdb/concepts/*.toml`, `Heuristic` for contexts synthesised from
    // crate-name prefix stripping during the per-crate loop. Pre-seeding
    // declared contexts FIRST means a later heuristic crate that maps to
    // the same context name cannot demote the source — `or_insert_with`
    // only fires when the entry is absent.
    let mut contexts_seen: BTreeMap<String, (ContextMeta, ContextSource)> =
        seed_declared_contexts(&overrides);

    // RFC-050 50-A: compute each workspace crate's `crate_tier` (topological
    // longest-path depth in the intra-workspace normal-`[dependencies]` DAG)
    // up front — the DAG needs the full member set, so this precedes the
    // per-crate emission loop. A cycle in the normal-deps DAG is fatal.
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

    // Step 2 (post-walk): emit one `:Context` node per unique bounded
    // context. `BTreeMap` iteration is ordered, so the emission order is
    // deterministic across runs regardless of which crate discovered the
    // context first. Contexts declared in `.cfdb/concepts/*.toml` that no
    // workspace crate is part of are still emitted — downstream tooling
    // may reference cross-workspace taxonomies.
    for (name, (meta, source)) in &contexts_seen {
        emit_context_node(&mut emitter, name, meta, *source);
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
    // Wrapper-unwrap third tier (#239, RFC-037 §6 closeout): when
    // exact-match + unique-last-segment miss on the outer rendered
    // return type, the resolver falls back to `render_type_inner` on
    // the original `syn::Type` (stored in the queue) with a
    // depth-3 recursion budget. This catches `fn f() -> Vec<Foo>` /
    // `Option<Foo>` / `Result<Ok, Err>` / nested combinations —
    // wrapper-wrapped same-crate types now emit RETURNS. The closed
    // 9-wrapper list lives in `type_render::WRAPPER_TYPES`.
    resolver::resolve_deferred_returns(&mut emitter);

    // Step 4 (post-walk) — TYPE_OF resolution (RFC-037 §3.4, #220;
    // #239). Same three-tier policy as RETURNS: exact-match, unique
    // last-segment fallback, and `render_type_inner` wrapper unwrap on
    // the stored `syn::Type`. Source labels in the deferred queue are
    // restricted to `:Field` and `:Param`; variant-level TYPE_OF is a
    // follow-up (variant payloads are already walked as `:Field`
    // nodes which queue their own TYPE_OF entries).
    resolver::resolve_deferred_type_of(&mut emitter);

    // Step 5 (post-walk) — synthesize minimal `:Item` nodes for edge dst
    // qnames that no walk path emitted: foreign traits (`std::fmt::Display`),
    // foreign types (`serde::Value`), or any same-workspace item referenced
    // by edge before walking. Without this pass `cfdb-petgraph::ingest_one_edge`
    // drops every IMPLEMENTS / IMPLEMENTS_FOR / RETURNS / TYPE_OF whose dst
    // is unknown — issue #317 (reframed from withdrawn RFC-039). Runs AFTER
    // RETURNS / TYPE_OF resolution so the resolvers' exact-match tier is
    // not contaminated by synthesised entries; runs BEFORE finish() so
    // synthesised nodes/edges land in the same canonical sort.
    synthesize::synthesize_referenced_items(&mut emitter, &overrides);

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
    crate_tiers: &BTreeMap<String, i64>,
    overrides: &ConceptOverrides,
    published_language: &PublishedLanguageCrates,
    contexts_seen: &mut BTreeMap<String, (ContextMeta, ContextSource)>,
    workspace_root: &Path,
) -> Result<(), ExtractError> {
    let crate_id = format!("crate:{}", package.name);
    let bounded_context = compute_bounded_context(&package.name, overrides).name;

    // Heuristic-synthesised contexts also need a `:Context` node so
    // `BELONGS_TO` has a valid target. The override-declared ones are
    // already pre-seeded in `contexts_seen` with `ContextSource::Declared`
    // (see `extract_workspace`); the helper only inserts a `Heuristic`
    // entry for names absent from the pre-seed. This implements the
    // §3.3 aggregation rule: a context declared via override cannot be
    // demoted by a later heuristic crate.
    accumulate_heuristic_context(contexts_seen, &bounded_context);

    emitter.emit_node(Node {
        id: crate_id.clone(),
        label: Label::new(Label::CRATE),
        props: {
            let mut p = BTreeMap::new();
            // RFC-050 50-A. `crate_tiers` is total over the workspace member
            // set, so the lookup always hits; `unwrap_or(0)` is a defensive
            // non-panic for the structurally-impossible miss (a leaf default).
            p.insert(
                "crate_tier".into(),
                PropValue::Int(
                    crate_tiers
                        .get(&package.name.to_string())
                        .copied()
                        .unwrap_or(0),
                ),
            );
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

/// Emit a single `:Context` node from its accumulated [`ContextMeta`] +
/// [`ContextSource`] discriminator (RFC-038 §3.3). Pulled out of the
/// context-emission loop so the per-property clones do not count against
/// the `clones-in-loops` metric.
fn emit_context_node(emitter: &mut Emitter, name: &str, meta: &ContextMeta, source: ContextSource) {
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
    props.insert(
        "source".into(),
        PropValue::Str(source.as_wire_str().to_string()),
    );
    emitter.emit_node(Node {
        id,
        label: Label::new(Label::CONTEXT),
        props,
    });
}

/// Build the per-context accumulator pre-seeded with every override-declared
/// context tagged [`ContextSource::Declared`]. RFC-038 §3.3 aggregation rule:
/// pre-seeding declared entries before the per-crate heuristic loop means
/// `or_insert_with` cannot demote a declared context to heuristic later on.
fn seed_declared_contexts(
    overrides: &ConceptOverrides,
) -> BTreeMap<String, (ContextMeta, ContextSource)> {
    overrides
        .declared_contexts()
        .into_iter()
        .map(|(name, meta)| (name, (meta, ContextSource::Declared)))
        .collect()
}

/// Insert a heuristic-synthesised context into the accumulator iff its name
/// is unseen. The entry-level idempotence is what makes the §3.3 aggregation
/// rule hold: a declared pre-seed for the same name suppresses this insert
/// entirely.
fn accumulate_heuristic_context(
    contexts_seen: &mut BTreeMap<String, (ContextMeta, ContextSource)>,
    name: &str,
) {
    contexts_seen.entry(name.to_string()).or_insert_with(|| {
        (
            ContextMeta {
                name: name.to_string(),
                canonical_crate: None,
                owning_rfc: None,
            },
            ContextSource::Heuristic,
        )
    });
}

#[cfg(test)]
mod tests;

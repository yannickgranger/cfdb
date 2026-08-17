//! Workspace-level node emission + per-package target walk — the
//! `:Crate` / `:Context` producers and the per-target `visit_file` dispatch.
//!
//! `extract_workspace` orchestration stays in `lib.rs`; this module owns
//! how workspace structure becomes nodes.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use cfdb_core::fact::{Edge, Node, PropValue};
use cfdb_core::qname::TargetDiscriminator;
use cfdb_core::schema::{EdgeLabel, Label};
use cfdb_core::ContextSource;

use cfdb_concepts::{
    compute_bounded_context, ConceptOverrides, ContextMeta, PublishedLanguageCrates,
};

use crate::emitter::Emitter;
use crate::file_walker::visit_file;
use crate::ExtractError;

/// Emit the `:Crate` node, `BELONGS_TO` edge, synthesised `:Context`
/// entries, and walk each lib/bin target for one workspace package.
/// Factored out of the crate-loop in [`crate::extract_workspace`] so the
/// per-package path-string and context-name clones live in a helper
/// rather than directly inside the outer `for` loop body.
pub(crate) fn emit_crate_and_walk_targets(
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
            // `crate_tiers` is total over the workspace member set, so the
            // lookup always hits; `unwrap_or(0)` is a defensive non-panic
            // for the structurally-impossible miss.
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
            // Published Language marker: `true` iff the crate is declared in
            // `.cfdb/published-language-crates.toml`. Every `:Crate` carries
            // this prop — no `Option`, missing file → `false`.
            p.insert(
                "published_language".into(),
                PropValue::Bool(published_language.is_published_language(&package.name)),
            );
            p
        },
    });

    // Emit the Crate -> Context BELONGS_TO edge now so a single pass
    // over edges shows the crate-to-context wiring.
    let context_id = format!("context:{bounded_context}");
    emitter.emit_edge(Edge {
        src: crate_id.clone(),
        dst: context_id,
        label: EdgeLabel::new(EdgeLabel::BELONGS_TO),
        props: BTreeMap::new(),
    });

    // Stop discarding target identity — each target root carries its
    // TargetDiscriminator so distinct cargo targets of one package occupy
    // distinct identity namespaces.
    let targets: Vec<(PathBuf, TargetDiscriminator)> = package
        .targets
        .iter()
        .filter(|t| t.is_lib() || t.is_bin())
        .map(|t| {
            let disc = if t.is_bin() {
                TargetDiscriminator::Bin {
                    name: t.name.clone(),
                }
            } else {
                TargetDiscriminator::Lib
            };
            (t.src_path.clone().into_std_path_buf(), disc)
        })
        .collect();
    for (src_root, target) in &targets {
        visit_file(
            emitter,
            &crate_id,
            &package.name,
            &bounded_context,
            target,
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
pub(crate) fn emit_context_node(
    emitter: &mut Emitter,
    name: &str,
    meta: &ContextMeta,
    source: ContextSource,
) {
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
pub(crate) fn seed_declared_contexts(
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
pub(crate) fn accumulate_heuristic_context(
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

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

    accumulate_heuristic_context(contexts_seen, &bounded_context);

    emitter.emit_node(Node {
        id: crate_id.clone(),
        label: Label::new(Label::CRATE),
        props: {
            let mut p = BTreeMap::new();
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
            p.insert(
                "published_language".into(),
                PropValue::Bool(published_language.is_published_language(&package.name)),
            );
            p
        },
    });

    let context_id = format!("context:{bounded_context}");
    emitter.emit_edge(Edge {
        src: crate_id.clone(),
        dst: context_id,
        label: EdgeLabel::new(EdgeLabel::BELONGS_TO),
        props: BTreeMap::new(),
    });

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

pub(crate) fn seed_declared_contexts(
    overrides: &ConceptOverrides,
) -> BTreeMap<String, (ContextMeta, ContextSource)> {
    overrides
        .declared_contexts()
        .into_iter()
        .map(|(name, meta)| (name, (meta, ContextSource::Declared)))
        .collect()
}

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

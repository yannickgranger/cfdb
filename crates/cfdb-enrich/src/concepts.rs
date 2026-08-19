use std::collections::BTreeMap;
use std::path::Path;

use cfdb_concepts::{load_concept_overrides, ConceptOverrides, ContextMeta};
use cfdb_core::enrich::EnrichReport;
use cfdb_core::fact::{Edge, Node, PropValue, Props};
use cfdb_core::graph::GraphView;
use cfdb_core::schema::{EdgeLabel, Label};

pub(crate) const VERB: &str = "enrich_concepts";
const ITEM_CRATE_PROP: &str = "crate";
const ASSIGNED_BY_MANUAL: &str = "manual";

pub(crate) fn run(view: &mut dyn GraphView, workspace_root: &Path) -> EnrichReport {
    let overrides = match load_concept_overrides(workspace_root) {
        Ok(o) => o,
        Err(e) => {
            return EnrichReport {
                verb: VERB.into(),
                ran: false,
                facts_scanned: 0,
                attrs_written: 0,
                edges_written: 0,
                warnings: vec![format!(
                    "{VERB}: failed to load `.cfdb/concepts/*.toml` under {workspace_root:?}: {e}"
                )],
            };
        }
    };

    let concepts = overrides.declared_contexts();
    if concepts.is_empty() {
        return EnrichReport {
            verb: VERB.into(),
            ran: true,
            facts_scanned: 0,
            attrs_written: 0,
            edges_written: 0,
            warnings: Vec::new(),
        };
    }

    let items_by_crate = build_item_index(view);

    let concept_nodes = build_concept_nodes(&concepts);
    let edges = build_edges(&overrides, &items_by_crate);

    let attrs_written: u64 = concept_nodes
        .iter()
        .map(|n| u64::try_from(n.props.len()).unwrap_or(u64::MAX))
        .sum();
    let edges_written = u64::try_from(edges.len()).unwrap_or(u64::MAX);
    let concepts_count = u64::try_from(concepts.len()).unwrap_or(u64::MAX);

    view.ingest_nodes(concept_nodes);
    view.ingest_edges(edges);

    EnrichReport {
        verb: VERB.into(),
        ran: true,
        facts_scanned: concepts_count,
        attrs_written,
        edges_written,
        warnings: Vec::new(),
    }
}

fn build_item_index(view: &dyn GraphView) -> BTreeMap<String, Vec<ItemRef>> {
    view.nodes_with_label(&Label::new(Label::ITEM))
        .into_iter()
        .filter_map(|id| {
            view.node_by_id(&id).and_then(|node| {
                node.props
                    .get(ITEM_CRATE_PROP)
                    .and_then(PropValue::as_str)
                    .map(|crate_name| (crate_name.to_string(), node.id.clone()))
            })
        })
        .fold(BTreeMap::new(), |mut acc, (crate_name, node_id)| {
            acc.entry(crate_name).or_default().push(ItemRef { node_id });
            acc
        })
}

struct ItemRef {
    node_id: String,
}

fn build_concept_nodes(concepts: &BTreeMap<String, ContextMeta>) -> Vec<Node> {
    concepts
        .values()
        .map(|meta| build_one_concept_node(&meta.name))
        .collect()
}

fn build_one_concept_node(name: &str) -> Node {
    let mut props = Props::new();
    props.insert("name".into(), PropValue::Str(name.to_string()));
    props.insert(
        "assigned_by".into(),
        PropValue::Str(ASSIGNED_BY_MANUAL.into()),
    );
    Node {
        id: concept_node_id(name),
        label: Label::new(Label::CONCEPT),
        props,
    }
}

fn concept_node_id(name: &str) -> String {
    format!("concept:{name}")
}

fn build_edges(
    overrides: &ConceptOverrides,
    items_by_crate: &BTreeMap<String, Vec<ItemRef>>,
) -> Vec<Edge> {
    let labeled_as = EdgeLabel::new(EdgeLabel::LABELED_AS);
    let canonical_for = EdgeLabel::new(EdgeLabel::CANONICAL_FOR);
    let canonical_by_concept = canonical_crates(overrides);

    let labeled_iter = overrides
        .crate_assignments()
        .iter()
        .flat_map(|(crate_name, meta)| {
            edges_for_crate(items_by_crate, crate_name, &meta.name, &labeled_as)
        });

    let canonical_iter = canonical_by_concept
        .iter()
        .flat_map(|(concept_name, canonical_crate)| {
            edges_for_crate(
                items_by_crate,
                canonical_crate,
                concept_name,
                &canonical_for,
            )
        });

    labeled_iter.chain(canonical_iter).collect()
}

fn edges_for_crate<'a>(
    items_by_crate: &'a BTreeMap<String, Vec<ItemRef>>,
    crate_name: &str,
    concept_name: &'a str,
    label: &'a EdgeLabel,
) -> impl Iterator<Item = Edge> + 'a {
    items_by_crate
        .get(crate_name)
        .into_iter()
        .flat_map(|items| items.iter())
        .map(move |item| Edge {
            src: item.node_id.clone(),
            dst: concept_node_id(concept_name),
            label: label.clone(),
            props: Props::new(),
        })
}

fn canonical_crates(overrides: &ConceptOverrides) -> BTreeMap<String, String> {
    overrides
        .crate_assignments()
        .values()
        .filter_map(|meta| {
            meta.canonical_crate
                .as_ref()
                .map(|c| (meta.name.clone(), c.clone()))
        })
        .fold(BTreeMap::new(), |mut acc, (name, canonical)| {
            acc.entry(name).or_insert(canonical);
            acc
        })
}

#[cfg(test)]
mod tests;

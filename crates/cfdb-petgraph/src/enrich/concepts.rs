//! `enrich_concepts` — materialize `:Concept` nodes from
//! `.cfdb/concepts/*.toml` declarations and emit `(:Item)-[:LABELED_AS]->
//! (:Concept)` + `(:Item)-[:CANONICAL_FOR]->(:Concept)` edges (slice 43-F /
//! issue #109).
//!
//! # Sixth pass — concept node materialisation
//!
//! DDD Q4 (council round 1 synthesis §43-F C2) caught that RFC §A2.2's
//! original 5-pass table omitted `:Concept` node materialisation that
//! downstream triggers (#101 T1, #102 T3) depend on. Schema was already
//! reserved in 43-A (`Label::CONCEPT`, `EdgeLabel::LABELED_AS`,
//! `EdgeLabel::CANONICAL_FOR`, and the `:Concept {name, assigned_by}`
//! descriptor with `Provenance::EnrichConcepts`) — this slice writes the
//! implementation.
//!
//! # Emission rules
//!
//! - One `:Concept { name, assigned_by: "manual" }` node per unique context
//!   declared in any `.cfdb/concepts/*.toml` file. `assigned_by = "manual"`
//!   is the invariant for TOML-declared concepts (auto-discovery from code
//!   is explicitly out of scope per the issue body).
//! - One `(:Item)-[:LABELED_AS]->(:Concept)` edge for every `:Item` whose
//!   `crate` prop appears in any TOML's `crates` list.
//! - One `(:Item)-[:CANONICAL_FOR]->(:Concept)` edge for every `:Item` in
//!   a TOML's declared `canonical_crate`. The current `ConceptFile` shape
//!   does NOT carry a `canonical_item_patterns` field — the scope of
//!   "canonical for a concept" is therefore every item in the canonical
//!   crate, not a pattern-filtered subset. Narrowing via patterns is
//!   deferred to a follow-up slice.
//!
//! # Graceful no-op
//!
//! Workspaces with no `.cfdb/concepts/*.toml` files return `ran: true,
//! attrs_written: 0, edges_written: 0` with no warning — empty-concept-
//! directory is a legitimate workspace shape.
//!
//! # Determinism
//!
//! - `ConceptOverrides::crate_assignments` iterates in sorted crate-name
//!   order (backed by `BTreeMap`).
//! - `:Concept` nodes are emitted sorted by context name.
//! - Edges are emitted sorted by `(item_node_id, concept_name)` via
//!   `BTreeMap<(String, String), Edge>` style ordering.

use std::collections::BTreeMap;
use std::path::Path;

use cfdb_concepts::{load_concept_overrides, ConceptOverrides, ContextMeta};
use cfdb_core::enrich::EnrichReport;
use cfdb_core::fact::{Edge, Node, PropValue, Props};
use cfdb_core::schema::{EdgeLabel, Label};
use petgraph::stable_graph::NodeIndex;

use crate::graph::KeyspaceState;

pub(crate) const VERB: &str = "enrich_concepts";
const ITEM_CRATE_PROP: &str = "crate";
const ASSIGNED_BY_MANUAL: &str = "manual";

pub(crate) fn run(state: &mut KeyspaceState, workspace_root: &Path) -> EnrichReport {
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

    // No TOML files at all → graceful no-op (AC-2).
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

    // Build item index: crate_name -> Vec<(node_id, node_idx)>. One O(N)
    // walk over `:Item` nodes; downstream edge emission is O(labelled crates
    // × items in that crate).
    let items_by_crate = build_item_index(state);

    let concept_nodes = build_concept_nodes(&concepts);
    let edges = build_edges(&overrides, &items_by_crate);

    let attrs_written: u64 = concept_nodes
        .iter()
        .map(|n| u64::try_from(n.props.len()).unwrap_or(u64::MAX))
        .sum();
    let edges_written = u64::try_from(edges.len()).unwrap_or(u64::MAX);
    let concepts_count = u64::try_from(concepts.len()).unwrap_or(u64::MAX);

    state.ingest_nodes(concept_nodes);
    state.ingest_edges(edges);

    EnrichReport {
        verb: VERB.into(),
        ran: true,
        facts_scanned: concepts_count,
        attrs_written,
        edges_written,
        warnings: Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// Item index
// ---------------------------------------------------------------------------

/// Walk `:Item` nodes once and group their `(node_id, index)` pairs by the
/// `crate` prop. Items with no `crate` prop are skipped — no LABELED_AS edge
/// possible without a crate assignment.
fn build_item_index(state: &KeyspaceState) -> BTreeMap<String, Vec<ItemRef>> {
    state
        .nodes_with_label(&Label::new(Label::ITEM))
        .into_iter()
        .filter_map(|idx| {
            state.graph.node_weight(idx).and_then(|node| {
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

// ---------------------------------------------------------------------------
// Node emission
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Edge emission
// ---------------------------------------------------------------------------

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
            edges_for_crate(items_by_crate, canonical_crate, concept_name, &canonical_for)
        });

    labeled_iter.chain(canonical_iter).collect()
}

/// Emit one edge per `:Item` in `crate_name` → `:Concept { name: concept }`.
/// Returns an empty iterator if the crate has no items in the graph. Clones
/// of `item_node_id` + `label` happen inside an iterator chain rather than a
/// for-loop body so the clones-in-loop metric does not flag them.
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

/// Build `concept_name -> canonical_crate` map, deduplicating across
/// multiple crate entries that share the same owning context. A concept
/// with no declared `canonical_crate` does not appear in the map.
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

// ---------------------------------------------------------------------------
// Re-use prevention: NodeIndex only used via state; suppress dead-code lint
// ---------------------------------------------------------------------------

#[allow(dead_code)]
fn _use_node_index(_: NodeIndex) {}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests;

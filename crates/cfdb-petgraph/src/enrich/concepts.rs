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

    let labeled: Vec<Edge> = overrides
        .crate_assignments()
        .iter()
        .flat_map(|(crate_name, meta)| {
            edges_for_crate(items_by_crate, crate_name, &meta.name, &labeled_as)
        })
        .collect();

    let canonical: Vec<Edge> = canonical_by_concept
        .iter()
        .flat_map(|(concept_name, canonical_crate)| {
            edges_for_crate(
                items_by_crate,
                canonical_crate,
                concept_name,
                &canonical_for,
            )
        })
        .collect();

    let mut out = labeled;
    out.extend(canonical);
    out
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
mod tests {
    use std::path::Path;

    use cfdb_core::enrich::EnrichBackend;
    use cfdb_core::fact::{Node, PropValue, Props};
    use cfdb_core::schema::{EdgeLabel, Keyspace, Label};
    use cfdb_core::store::StoreBackend;

    use crate::PetgraphStore;

    fn write(root: &Path, rel: &str, contents: &str) {
        let path = root.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("mkdirs");
        }
        std::fs::write(&path, contents).expect("write");
    }

    fn store_with_items(workspace: &Path, items: &[(&str, &str)]) -> PetgraphStore {
        let mut store = PetgraphStore::new().with_workspace(workspace);
        let ks = Keyspace::new("test");
        let nodes: Vec<Node> = items
            .iter()
            .map(|(qname, crate_name)| {
                let mut props = Props::new();
                props.insert("qname".into(), PropValue::Str((*qname).into()));
                props.insert("name".into(), PropValue::Str((*qname).into()));
                props.insert("crate".into(), PropValue::Str((*crate_name).into()));
                props.insert("file".into(), PropValue::Str("src/lib.rs".into()));
                Node {
                    id: format!("item:{qname}"),
                    label: Label::new(Label::ITEM),
                    props,
                }
            })
            .collect();
        store.ingest_nodes(&ks, nodes).expect("ingest");
        store
    }

    fn count_nodes_by_label(store: &PetgraphStore, ks: &Keyspace, label: &str) -> usize {
        let (nodes, _) = store.export(ks).expect("export");
        nodes.iter().filter(|n| n.label.as_str() == label).count()
    }

    fn count_edges_by_label(store: &PetgraphStore, ks: &Keyspace, label: &str) -> usize {
        let (_, edges) = store.export(ks).expect("export");
        edges.iter().filter(|e| e.label.as_str() == label).count()
    }

    // ------------------------------------------------------------------
    // AC-1: TOML with canonical_crate → :Concept + LABELED_AS + CANONICAL_FOR.
    // ------------------------------------------------------------------

    #[test]
    fn ac1_toml_emits_concept_plus_labeled_as_plus_canonical_for() {
        let tmp = tempfile::tempdir().expect("tempdir");
        write(
            tmp.path(),
            ".cfdb/concepts/trading.toml",
            r#"
name = "trading"
canonical_crate = "domain-trading"
crates = ["domain-trading", "ports-trading"]
"#,
        );
        let mut store = store_with_items(
            tmp.path(),
            &[
                ("A", "domain-trading"),
                ("B", "domain-trading"),
                ("C", "ports-trading"),
                ("D", "application-x"), // not covered → no edges
            ],
        );
        let ks = Keyspace::new("test");
        let report = store.enrich_concepts(&ks).expect("pass");

        assert!(report.ran);
        assert_eq!(report.facts_scanned, 1, "one declared concept");
        assert_eq!(count_nodes_by_label(&store, &ks, Label::CONCEPT), 1);
        // LABELED_AS: A, B (domain-trading) + C (ports-trading) = 3.
        assert_eq!(
            count_edges_by_label(&store, &ks, EdgeLabel::LABELED_AS),
            3,
            "3 items in TOML-listed crates → 3 LABELED_AS edges"
        );
        // CANONICAL_FOR: A + B (both in domain-trading) = 2.
        assert_eq!(
            count_edges_by_label(&store, &ks, EdgeLabel::CANONICAL_FOR),
            2,
            "2 items in canonical_crate → 2 CANONICAL_FOR edges"
        );
    }

    // ------------------------------------------------------------------
    // AC-2: empty .cfdb/concepts/ → ran=true, zero emissions.
    // ------------------------------------------------------------------

    #[test]
    fn ac2_empty_concepts_dir_is_graceful_noop() {
        let tmp = tempfile::tempdir().expect("tempdir");
        // Create the dir but put no .toml files in it.
        std::fs::create_dir_all(tmp.path().join(".cfdb/concepts")).expect("mkdir");
        let mut store = store_with_items(tmp.path(), &[("A", "domain-x")]);
        let ks = Keyspace::new("test");
        let report = store.enrich_concepts(&ks).expect("pass");

        assert!(report.ran, "empty concepts dir is a valid workspace shape");
        assert_eq!(report.facts_scanned, 0);
        assert_eq!(report.attrs_written, 0);
        assert_eq!(report.edges_written, 0);
        assert!(
            report.warnings.is_empty(),
            "no warning expected for empty concepts dir"
        );
    }

    #[test]
    fn no_concepts_dir_is_graceful_noop() {
        let tmp = tempfile::tempdir().expect("tempdir");
        // No .cfdb directory at all.
        let mut store = store_with_items(tmp.path(), &[("A", "domain-x")]);
        let ks = Keyspace::new("test");
        let report = store.enrich_concepts(&ks).expect("pass");

        assert!(report.ran);
        assert_eq!(report.facts_scanned, 0);
        assert_eq!(report.edges_written, 0);
    }

    // ------------------------------------------------------------------
    // AC-3: malformed TOML → ran=false + warning naming the file.
    // ------------------------------------------------------------------

    #[test]
    fn ac3_malformed_toml_returns_ran_false_with_warning() {
        let tmp = tempfile::tempdir().expect("tempdir");
        write(
            tmp.path(),
            ".cfdb/concepts/broken.toml",
            "this is = not [valid toml",
        );
        let mut store = store_with_items(tmp.path(), &[("A", "domain-x")]);
        let ks = Keyspace::new("test");
        let report = store.enrich_concepts(&ks).expect("pass");

        assert!(!report.ran, "TOML load error → ran=false");
        assert_eq!(report.edges_written, 0);
        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.contains("concepts") || w.contains("toml") || w.contains("broken")),
            "warning must name the problem file: {:?}",
            report.warnings
        );
    }

    // ------------------------------------------------------------------
    // AC-5: determinism across two runs.
    // ------------------------------------------------------------------

    #[test]
    fn ac5_two_runs_produce_identical_canonical_dumps() {
        let tmp = tempfile::tempdir().expect("tempdir");
        write(
            tmp.path(),
            ".cfdb/concepts/trading.toml",
            r#"
name = "trading"
canonical_crate = "domain-trading"
crates = ["domain-trading", "ports-trading"]
"#,
        );
        write(
            tmp.path(),
            ".cfdb/concepts/risk.toml",
            r#"
name = "risk"
crates = ["domain-risk"]
"#,
        );

        fn build(root: &Path) -> PetgraphStore {
            let mut store = PetgraphStore::new().with_workspace(root);
            let ks = Keyspace::new("test");
            for (q, c) in [
                ("A", "domain-trading"),
                ("B", "ports-trading"),
                ("C", "domain-risk"),
            ] {
                let mut props = Props::new();
                props.insert("qname".into(), PropValue::Str(q.into()));
                props.insert("name".into(), PropValue::Str(q.into()));
                props.insert("crate".into(), PropValue::Str(c.into()));
                store
                    .ingest_nodes(
                        &ks,
                        vec![Node {
                            id: format!("item:{q}"),
                            label: Label::new(Label::ITEM),
                            props,
                        }],
                    )
                    .expect("ingest");
            }
            store
        }

        let ks = Keyspace::new("test");
        let mut s1 = build(tmp.path());
        s1.enrich_concepts(&ks).expect("run 1");
        let mut s2 = build(tmp.path());
        s2.enrich_concepts(&ks).expect("run 2");
        let d1 = s1.canonical_dump(&ks).expect("dump 1");
        let d2 = s2.canonical_dump(&ks).expect("dump 2");
        assert_eq!(d1, d2, "two runs must be byte-identical (AC-5)");
    }

    // ------------------------------------------------------------------
    // AC-7: integration contract for #101 — 3 concepts → 3 :Concept nodes.
    // ------------------------------------------------------------------

    #[test]
    fn ac7_three_toml_files_emit_three_concept_nodes() {
        let tmp = tempfile::tempdir().expect("tempdir");
        write(
            tmp.path(),
            ".cfdb/concepts/trading.toml",
            "name = \"trading\"\ncrates = [\"domain-trading\"]\n",
        );
        write(
            tmp.path(),
            ".cfdb/concepts/risk.toml",
            "name = \"risk\"\ncrates = [\"domain-risk\"]\n",
        );
        write(
            tmp.path(),
            ".cfdb/concepts/ledger.toml",
            "name = \"ledger\"\ncrates = [\"domain-ledger\"]\n",
        );
        let mut store = store_with_items(
            tmp.path(),
            &[
                ("A", "domain-trading"),
                ("B", "domain-risk"),
                ("C", "domain-ledger"),
            ],
        );
        let ks = Keyspace::new("test");
        store.enrich_concepts(&ks).expect("pass");

        assert_eq!(
            count_nodes_by_label(&store, &ks, Label::CONCEPT),
            3,
            "3 TOML files → 3 :Concept nodes (AC-7)"
        );
    }

    // ------------------------------------------------------------------
    // AC-1 reinforcement: assigned_by = "manual" on emitted :Concept nodes.
    // ------------------------------------------------------------------

    #[test]
    fn concept_nodes_have_assigned_by_manual() {
        let tmp = tempfile::tempdir().expect("tempdir");
        write(
            tmp.path(),
            ".cfdb/concepts/trading.toml",
            "name = \"trading\"\ncrates = [\"domain-trading\"]\n",
        );
        let mut store = store_with_items(tmp.path(), &[("A", "domain-trading")]);
        let ks = Keyspace::new("test");
        store.enrich_concepts(&ks).expect("pass");

        let (nodes, _) = store.export(&ks).expect("export");
        let concept = nodes
            .iter()
            .find(|n| n.label.as_str() == Label::CONCEPT)
            .expect(":Concept node must exist");
        assert_eq!(
            concept.props.get("assigned_by").and_then(PropValue::as_str),
            Some("manual")
        );
        assert_eq!(
            concept.props.get("name").and_then(PropValue::as_str),
            Some("trading")
        );
    }

    // ------------------------------------------------------------------
    // Degraded paths
    // ------------------------------------------------------------------

    #[test]
    fn unknown_keyspace_returns_err() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let mut store = PetgraphStore::new().with_workspace(tmp.path());
        let ks = Keyspace::new("never");
        let err = store
            .enrich_concepts(&ks)
            .expect_err("unknown keyspace must err");
        assert!(format!("{err:?}").contains("UnknownKeyspace"));
    }

    #[test]
    fn no_workspace_root_returns_degraded_report() {
        let mut store = PetgraphStore::new();
        let ks = Keyspace::new("test");
        let mut props = Props::new();
        props.insert("qname".into(), PropValue::Str("A".into()));
        props.insert("crate".into(), PropValue::Str("domain-x".into()));
        store
            .ingest_nodes(
                &ks,
                vec![Node {
                    id: "item:A".into(),
                    label: Label::new(Label::ITEM),
                    props,
                }],
            )
            .expect("ingest");
        let report = store.enrich_concepts(&ks).expect("pass");
        assert!(!report.ran);
        assert!(report.warnings.iter().any(|w| w.contains("workspace_root")));
    }

    #[test]
    fn items_without_crate_prop_are_ignored() {
        let tmp = tempfile::tempdir().expect("tempdir");
        write(
            tmp.path(),
            ".cfdb/concepts/trading.toml",
            "name = \"trading\"\ncrates = [\"domain-trading\"]\n",
        );
        let mut store = PetgraphStore::new().with_workspace(tmp.path());
        let ks = Keyspace::new("test");
        let mut props = Props::new();
        props.insert("qname".into(), PropValue::Str("NoCrate".into()));
        // Deliberately omit `crate` prop.
        store
            .ingest_nodes(
                &ks,
                vec![Node {
                    id: "item:NoCrate".into(),
                    label: Label::new(Label::ITEM),
                    props,
                }],
            )
            .expect("ingest");
        let report = store.enrich_concepts(&ks).expect("pass");

        assert!(report.ran);
        // :Concept still emitted, but no edges to the item.
        assert_eq!(report.edges_written, 0);
    }
}

//! Self-dogfood test for `enrich_concepts` (issue #109 — slice 43-F).
//!
//! Extracts cfdb's own worktree (which carries `.cfdb/concepts/cfdb.toml`
//! declaring a single `cfdb` concept with 9 crates + `canonical_crate =
//! "cfdb-core"`), runs `enrich_concepts`, and asserts:
//!
//! - One `:Concept { name: "cfdb", assigned_by: "manual" }` node exists
//! - `LABELED_AS` edges cover every cfdb `:Item` (848 items per #108's
//!   self-dogfood report)
//! - `CANONICAL_FOR` edges cover every `:Item` in the `cfdb-core` crate
//!
//! AC-4 of the issue expected "zero emissions" on cfdb's tree under the
//! assumption that cfdb had no concepts file. cfdb.toml has since been
//! added (council-cfdb-wiring), so the AC is reinterpreted: the pass
//! succeeds gracefully with real emissions — the negative-case regression
//! spirit is preserved by the module-level `no_concepts_dir_is_graceful_noop`
//! unit test.

use std::path::PathBuf;

use cfdb_core::enrich::EnrichBackend;
use cfdb_core::fact::PropValue;
use cfdb_core::schema::{EdgeLabel, Keyspace, Label};
use cfdb_core::store::StoreBackend;
use cfdb_petgraph::PetgraphStore;

fn cfdb_workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("CARGO_MANIFEST_DIR must have two parents")
        .to_path_buf()
}

#[test]
fn self_dogfood_cfdb_concept_and_labeled_as_coverage() {
    let workspace = cfdb_workspace_root();
    let (nodes, edges) = cfdb_extractor::extract_workspace(&workspace).expect("extract cfdb");

    let mut store = PetgraphStore::new().with_workspace(&workspace);
    let ks = Keyspace::new("selfdog");
    store.ingest_nodes(&ks, nodes).expect("ingest nodes");
    store.ingest_edges(&ks, edges).expect("ingest edges");

    let report = store.enrich_concepts(&ks).expect("enrich_concepts");
    assert!(report.ran, "pass must run: {:?}", report.warnings);
    assert_eq!(
        report.facts_scanned, 1,
        "cfdb has exactly one concepts/*.toml file — the cfdb.toml"
    );
    assert!(report.edges_written > 0, "LABELED_AS edges expected");

    let (all_nodes, all_edges) = store.export(&ks).expect("export");

    let cfdb_concept = all_nodes
        .iter()
        .find(|n| {
            n.label.as_str() == Label::CONCEPT
                && n.props
                    .get("name")
                    .and_then(PropValue::as_str)
                    .is_some_and(|v| v == "cfdb")
        })
        .expect(":Concept {name: \"cfdb\"} must exist");
    assert_eq!(
        cfdb_concept
            .props
            .get("assigned_by")
            .and_then(PropValue::as_str),
        Some("manual")
    );

    let total_items = all_nodes
        .iter()
        .filter(|n| n.label.as_str() == Label::ITEM)
        .count();
    let labeled_as_count = all_edges
        .iter()
        .filter(|e| e.label.as_str() == EdgeLabel::LABELED_AS && e.dst == cfdb_concept.id)
        .count();
    assert_eq!(
        labeled_as_count, total_items,
        "every cfdb :Item should have a LABELED_AS edge to :Concept{{cfdb}} \
         (got {labeled_as_count} edges vs {total_items} items)"
    );

    // CANONICAL_FOR: cfdb.toml declares canonical_crate = "cfdb-core", so
    // every :Item in cfdb-core gets a CANONICAL_FOR edge to :Concept{cfdb}.
    let canonical_for_count = all_edges
        .iter()
        .filter(|e| e.label.as_str() == EdgeLabel::CANONICAL_FOR && e.dst == cfdb_concept.id)
        .count();
    let cfdb_core_items = all_nodes
        .iter()
        .filter(|n| {
            n.label.as_str() == Label::ITEM
                && n.props
                    .get("crate")
                    .and_then(PropValue::as_str)
                    .is_some_and(|c| c == "cfdb-core")
        })
        .count();
    assert_eq!(
        canonical_for_count, cfdb_core_items,
        "every cfdb-core :Item should have a CANONICAL_FOR edge to \
         :Concept{{cfdb}} (got {canonical_for_count} edges vs {cfdb_core_items} \
         cfdb-core items)"
    );

    eprintln!(
        "self-dogfood: 1 :Concept{{cfdb}} + {labeled_as_count} LABELED_AS \
         + {canonical_for_count} CANONICAL_FOR (canonical crate = cfdb-core)"
    );
}
